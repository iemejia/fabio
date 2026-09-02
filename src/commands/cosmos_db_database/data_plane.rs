//! Cosmos DB **data-plane** transport (the `NoSQL` REST API).
//!
//! Fabric's Cosmos DB uses the same engine as Azure Cosmos DB for `NoSQL`, so
//! fabio speaks the documented `NoSQL` REST data-plane directly. Every request
//! goes through the raw [`FabricClient::http`] client (like the Kusto/KQL path)
//! because the endpoint is a per-item host (`*.cosmos.fabric.microsoft.com`),
//! not the Fabric REST base URL.
//!
//! Authentication is **Microsoft Entra ID only** — the Cosmos REST authorization
//! header for an AAD token is `type=aad&ver=1.0&sig=<jwt>` (URL-encoded), which
//! needs no per-request HMAC signing (unlike master-key auth). The token is
//! acquired for the `https://cosmos.azure.com/.default` scope.
//!
//! This module is the ONE Cosmos transport — `containers` and `documents`
//! consume it and never build raw Cosmos requests themselves.

use anyhow::Result;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Method, StatusCode};
use serde_json::Value;

use crate::client::{self, FabricClient};
use crate::errors::{ErrorCode, FabioError};

/// Cosmos `NoSQL` REST API version pinned by fabio.
const COSMOS_API_VERSION: &str = "2018-12-31";

/// A resolved Cosmos DB data-plane target: the per-item endpoint host, the
/// database name (= the Fabric item display name), and a cached AAD token.
///
/// Owns a cloned [`FabricClient`] (cheap, `Arc`-backed) so it can be moved into
/// `'static` parallel-import tasks.
pub(super) struct CosmosClient {
    client: FabricClient,
    /// Endpoint host, no trailing slash (e.g. `https://<id>.z64.sql.cosmos.fabric.microsoft.com`).
    endpoint: String,
    /// Cosmos database name — equals the Fabric item's display name.
    database: String,
    /// URL-encoded `type=aad&ver=1.0&sig=<token>` authorization header value.
    auth_header: String,
}

/// Parsed response from a Cosmos data-plane call.
pub(super) struct CosmosResponse {
    /// The JSON body.
    pub body: Value,
    /// Request-unit charge (`x-ms-request-charge`), if reported.
    pub request_charge: Option<f64>,
    /// Continuation token (`x-ms-continuation`) for paged queries, if any.
    pub continuation: Option<String>,
}

impl CosmosClient {
    /// Resolve the data-plane endpoint + database name for a Cosmos DB item and
    /// acquire an AAD token.
    ///
    /// The endpoint is read from the item's `properties.serverFqdn`
    /// (validated as a trusted `*.fabric.microsoft.com` host) unless
    /// `endpoint_override` is supplied. The database name is
    /// `properties.databaseName` (the item display name).
    pub(super) async fn connect(
        client: &FabricClient,
        workspace: &str,
        id: &str,
        endpoint_override: Option<&str>,
    ) -> Result<Self> {
        let data = client
            .get(&format!("/workspaces/{workspace}/cosmosDbDatabases/{id}"))
            .await
            .map_err(|e| crate::errors::enrich_forbidden(e, "cosmos-db-database", "Viewer"))?;

        let properties = data.get("properties");
        let database = properties
            .and_then(|p| p.get("databaseName"))
            .and_then(Value::as_str)
            .or_else(|| data.get("displayName").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();

        let endpoint = if let Some(uri) = endpoint_override {
            client::validate_trusted_url(uri, "--endpoint")?;
            uri.trim_end_matches('/').to_string()
        } else {
            let fqdn = properties
                .and_then(|p| p.get("serverFqdn"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    FabioError::with_hint(
                        ErrorCode::NotFound,
                        "Could not determine the Cosmos DB NoSQL endpoint from item properties."
                            .to_string(),
                        "Provide it explicitly with --endpoint. Find it in the Fabric portal: \
                         Cosmos DB database → Settings → Connection → 'Endpoint for Cosmos DB \
                         NoSQL database'.",
                    )
                })?;
            client::validate_trusted_url(fqdn, "serverFqdn (from item properties)")?;
            fqdn.trim_end_matches('/').to_string()
        };

        let token = client
            .require_token_for_scope(client::cosmos_scope())
            .await?;
        let auth_header = aad_auth_header(&token);

        Ok(Self {
            client: client.clone(),
            endpoint,
            database,
            auth_header,
        })
    }

    /// Reject a mutating call when `--readonly` is active. The raw `http()`
    /// client bypasses the built-in `guard_readonly`, so mutating helpers must
    /// call this explicitly.
    fn guard_readonly(&self, method: &str, resource_link: &str) -> Result<()> {
        if self.client.is_readonly() {
            return Err(FabioError::with_hint(
                ErrorCode::ReadonlyMode,
                format!("Blocked Cosmos {method} on {resource_link} — readonly mode is active"),
                "Remove --readonly flag or set FABIO_READONLY=0 to allow mutations.",
            )
            .into());
        }
        Ok(())
    }

    /// Send a Cosmos data-plane request and parse the response.
    ///
    /// `resource_path` is appended to the endpoint host (e.g.
    /// `/dbs/{db}/colls`). `extra_headers` carries Cosmos-specific headers
    /// (partition key, query flags, upsert, autoscale).
    async fn send(
        &self,
        method: Method,
        resource_path: &str,
        content_type: Option<&str>,
        extra_headers: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<CosmosResponse> {
        let url = format!("{}{resource_path}", self.endpoint);
        let date = rfc1123_now();
        let mut req = self
            .client
            .http()
            .request(method, &url)
            .header(AUTHORIZATION, &self.auth_header)
            .header("x-ms-date", &date)
            .header("x-ms-version", COSMOS_API_VERSION)
            .header(ACCEPT, "application/json");
        if let Some(ct) = content_type {
            req = req.header(CONTENT_TYPE, ct);
        }
        for (k, v) in extra_headers {
            req = req.header(*k, v);
        }
        if let Some(b) = body {
            req = req.body(serde_json::to_vec(b)?);
        }

        let resp = req.send().await.map_err(|e| {
            FabioError::new(
                ErrorCode::NetworkError,
                format!("Cosmos request failed: {e}"),
            )
        })?;
        let status = resp.status();
        let request_charge = header_f64(&resp, "x-ms-request-charge");
        let continuation = header_string(&resp, "x-ms-continuation");
        let text = resp.text().await.unwrap_or_default();
        let value: Value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text.clone()))
        };

        if !status.is_success() {
            return Err(cosmos_error(status, &value, &text).into());
        }

        Ok(CosmosResponse {
            body: value,
            request_charge,
            continuation,
        })
    }

    // ── Container operations ────────────────────────────────────────────────

    /// `GET /dbs/{db}/colls` — list containers. Returns the `DocumentCollections` array.
    pub(super) async fn list_containers(&self) -> Result<CosmosResponse> {
        let path = format!("/dbs/{}/colls", enc(&self.database));
        self.send(Method::GET, &path, None, &[], None).await
    }

    /// `GET /dbs/{db}/colls/{coll}` — read one container's definition.
    pub(super) async fn get_container(&self, container: &str) -> Result<CosmosResponse> {
        let path = format!("/dbs/{}/colls/{}", enc(&self.database), enc(container));
        self.send(Method::GET, &path, None, &[], None).await
    }

    /// `POST /dbs/{db}/colls` — create a container. Fabric Cosmos is
    /// autoscale-only, so an autopilot (autoscale) throughput header is always sent.
    pub(super) async fn create_container(
        &self,
        container: &str,
        partition_key_path: &str,
        autoscale_max_throughput: u32,
        default_ttl: Option<i64>,
    ) -> Result<CosmosResponse> {
        self.guard_readonly("POST", &format!("colls/{container}"))?;
        let path = format!("/dbs/{}/colls", enc(&self.database));
        let mut body = serde_json::json!({
            "id": container,
            "partitionKey": {
                "paths": [partition_key_path],
                "kind": "Hash",
                "version": 2
            }
        });
        if let Some(ttl) = default_ttl {
            body["defaultTtl"] = Value::from(ttl);
        }
        let autopilot = format!("{{\"maxThroughput\":{autoscale_max_throughput}}}");
        self.send(
            Method::POST,
            &path,
            Some("application/json"),
            &[("x-ms-cosmos-offer-autopilot-settings", autopilot)],
            Some(&body),
        )
        .await
    }

    /// `DELETE /dbs/{db}/colls/{coll}` — delete a container and all its documents.
    pub(super) async fn delete_container(&self, container: &str) -> Result<CosmosResponse> {
        self.guard_readonly("DELETE", &format!("colls/{container}"))?;
        let path = format!("/dbs/{}/colls/{}", enc(&self.database), enc(container));
        self.send(Method::DELETE, &path, None, &[], None).await
    }

    // ── Document operations ─────────────────────────────────────────────────

    /// `POST /dbs/{db}/colls/{coll}/docs` with query headers — run a `NoSQL` query.
    ///
    /// When `partition_key` is `None` the query is enabled for cross-partition
    /// execution automatically (ad-hoc queries span all partitions); passing a
    /// key scopes the query to that single partition.
    pub(super) async fn query(
        &self,
        container: &str,
        query_text: &str,
        parameters: &[Value],
        partition_key: Option<&Value>,
        max_item_count: Option<u32>,
        continuation: Option<&str>,
    ) -> Result<CosmosResponse> {
        let path = format!("/dbs/{}/colls/{}/docs", enc(&self.database), enc(container));
        let body = serde_json::json!({ "query": query_text, "parameters": parameters });
        let mut headers: Vec<(&str, String)> = vec![
            ("x-ms-documentdb-isquery", "true".to_string()),
            (
                "x-ms-max-item-count",
                max_item_count.unwrap_or(100).to_string(),
            ),
        ];
        if let Some(pk) = partition_key {
            headers.push(("x-ms-documentdb-partitionkey", format!("[{pk}]")));
        } else {
            headers.push((
                "x-ms-documentdb-query-enablecrosspartition",
                "true".to_string(),
            ));
        }
        if let Some(token) = continuation {
            headers.push(("x-ms-continuation", token.to_string()));
        }
        self.send(
            Method::POST,
            &path,
            Some("application/query+json"),
            &headers,
            Some(&body),
        )
        .await
    }

    /// `POST /dbs/{db}/colls/{coll}/docs` — create or upsert a single document.
    pub(super) async fn write_document(
        &self,
        container: &str,
        document: &Value,
        partition_key: &Value,
        upsert: bool,
    ) -> Result<CosmosResponse> {
        self.guard_readonly("POST", &format!("colls/{container}/docs"))?;
        let path = format!("/dbs/{}/colls/{}/docs", enc(&self.database), enc(container));
        let mut headers: Vec<(&str, String)> =
            vec![("x-ms-documentdb-partitionkey", format!("[{partition_key}]"))];
        if upsert {
            headers.push(("x-ms-documentdb-is-upsert", "true".to_string()));
        }
        self.send(
            Method::POST,
            &path,
            Some("application/json"),
            &headers,
            Some(document),
        )
        .await
    }
}

/// Build the Cosmos AAD authorization header value:
/// URL-encoded `type=aad&ver=1.0&sig=<token>`.
fn aad_auth_header(token: &str) -> String {
    urlencoding::encode(&format!("type=aad&ver=1.0&sig={token}")).into_owned()
}

/// Current UTC time formatted as an RFC 1123 / HTTP-date string
/// (e.g. `Tue, 01 Nov 1994 08:12:31 GMT`), as required by `x-ms-date`.
fn rfc1123_now() -> String {
    chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string()
}

/// URL-encode a single Cosmos resource path segment (db/container/document id).
fn enc(segment: &str) -> String {
    urlencoding::encode(segment).into_owned()
}

fn header_f64(resp: &reqwest::Response, name: &str) -> Option<f64> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok())
}

fn header_string(resp: &reqwest::Response, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

/// Map a Cosmos data-plane error response to a `FabioError` with a stable code.
fn cosmos_error(status: StatusCode, body: &Value, raw: &str) -> FabioError {
    let code = match status {
        StatusCode::UNAUTHORIZED => ErrorCode::AuthRequired,
        StatusCode::FORBIDDEN => ErrorCode::Forbidden,
        StatusCode::NOT_FOUND => ErrorCode::NotFound,
        StatusCode::CONFLICT => ErrorCode::Conflict,
        StatusCode::TOO_MANY_REQUESTS => ErrorCode::RateLimited,
        StatusCode::BAD_REQUEST => ErrorCode::InvalidInput,
        _ => ErrorCode::ApiError,
    };
    let message = body.get("message").and_then(Value::as_str).map_or_else(
        || raw.trim().to_string(),
        |m| m.lines().next().unwrap_or(m).to_string(),
    );
    FabioError::new(
        code,
        format!("Cosmos DB error ({}): {message}", status.as_u16()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aad_header_is_url_encoded() {
        let h = aad_auth_header("abc.def-ghi");
        assert_eq!(h, "type%3Daad%26ver%3D1.0%26sig%3Dabc.def-ghi");
        // The '=' and '&' separators must be encoded so the header parses.
        assert!(!h.contains('='));
        assert!(!h.contains('&'));
    }

    #[test]
    fn rfc1123_has_gmt_and_weekday() {
        let d = rfc1123_now();
        assert!(d.ends_with(" GMT"), "date must end with GMT: {d}");
        // "Www, DD Mon YYYY HH:MM:SS GMT" == 29 chars.
        assert_eq!(d.len(), 29, "unexpected RFC1123 length: {d}");
        assert_eq!(&d[3..5], ", ");
    }

    #[test]
    fn cosmos_error_maps_status_to_code() {
        let body = serde_json::json!({"code": "NotFound", "message": "Entity not found\r\ndetail"});
        let err = cosmos_error(StatusCode::NOT_FOUND, &body, "");
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(err.message.contains("Entity not found"));
        assert!(!err.message.contains("detail"), "only first line kept");
    }
}
