//! Bounded, non-recursive `OneLake` metadata reads. No arbitrary returned URLs.

use anyhow::Result;
use reqwest::header::AUTHORIZATION;
use reqwest::{Response, StatusCode};
use serde_json::Value;

use super::{FabricClient, encode_onelake_path, error_from_status_body, validate_uuid};
use crate::errors::{ErrorCode, FabioError};
use crate::verbose;

pub struct MetadataPage {
    pub body: Value,
    pub continuation: Option<String>,
    pub bytes_read: usize,
}

impl FabricClient {
    /// One shared authenticated GET for existing downloads and bounded metadata
    /// readers. Only authentication challenges are retried, never mutations.
    pub(super) async fn onelake_get_response(&self, url: &str) -> Result<Response> {
        verbose::trace_request("GET", url, None);
        let started = std::time::Instant::now();
        let mut response = self
            .http
            .get(url)
            .header(AUTHORIZATION, self.require_storage_auth().await?)
            .send()
            .await
            .map_err(|error| FabioError::new(ErrorCode::NetworkError, error.to_string()))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_storage_token().await;
            response = self
                .http
                .get(url)
                .header(AUTHORIZATION, self.require_storage_auth().await?)
                .send()
                .await
                .map_err(|error| FabioError::new(ErrorCode::NetworkError, error.to_string()))?;
        }
        verbose::trace_response(
            response.status().as_u16(),
            url,
            started.elapsed().as_millis(),
        );
        Ok(response)
    }

    /// A single non-recursive DFS directory page with explicit row/body caps.
    #[allow(clippy::too_many_arguments)]
    pub async fn list_onelake_metadata_page(
        &self,
        workspace: &str,
        item: &str,
        directory: &str,
        token: Option<&str>,
        page_size: u16,
        byte_limit: usize,
    ) -> Result<MetadataPage> {
        validate_uuid(workspace, "workspace")?;
        validate_uuid(item, "item")?;
        let directory = format!("{item}/{directory}");
        let mut query = format!(
            "?resource=filesystem&recursive=false&maxResults={page_size}&directory={}",
            urlencoding::encode(&directory)
        );
        if let Some(token) = token {
            query.push_str("&continuation=");
            query.push_str(&urlencoding::encode(token));
        }
        let response = self
            .onelake_get_response(&self.onelake_dfs_url(workspace, &query))
            .await?;
        let mut pointers = response.headers().get_all("x-ms-continuation").iter();
        let continuation = pointers.next().map(|value| {
            value
                .to_str()
                .ok()
                .filter(|value| value.len() <= 16_384)
                .map(str::to_string)
        });
        if pointers.next().is_some() || continuation.as_ref().is_some_and(Option::is_none) {
            return Err(FabioError::new(
                ErrorCode::ApiError,
                "Malformed or duplicate OneLake continuation metadata",
            )
            .into());
        }
        let bytes = bounded_bytes(response, byte_limit).await?;
        let body = serde_json::from_slice(&bytes)
            .map_err(|_| FabioError::new(ErrorCode::ApiError, "Malformed OneLake metadata page"))?;
        Ok(MetadataPage {
            body,
            continuation: continuation.flatten().filter(|value| !value.is_empty()),
            bytes_read: bytes.len(),
        })
    }

    pub async fn download_onelake_metadata_file(
        &self,
        workspace: &str,
        item: &str,
        path: &str,
        byte_limit: usize,
    ) -> Result<Vec<u8>> {
        validate_uuid(workspace, "workspace")?;
        validate_uuid(item, "item")?;
        let url = self.onelake_dfs_url(workspace, &format!("{item}/{}", encode_onelake_path(path)));
        bounded_bytes(self.onelake_get_response(&url).await?, byte_limit).await
    }
}

async fn bounded_bytes(mut response: Response, limit: usize) -> Result<Vec<u8>> {
    let oversized = || {
        FabioError::with_hint(
            ErrorCode::ApiError,
            "OneLake metadata exceeded the remaining body-byte budget.",
            "Evidence remains unknown; use explicit bounded source-check limits rather than assuming compatibility.",
        )
    };
    if limit == 0
        || response
            .content_length()
            .is_some_and(|length| length > u64::try_from(limit).unwrap_or(u64::MAX))
    {
        return Err(oversized().into());
    }
    let status = response.status();
    let range = response.headers().get("content-range");
    let complete_length = range
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (span, total) = value.strip_prefix("bytes ")?.split_once('/')?;
            let (start, end) = span.split_once('-')?;
            let total = total.parse::<usize>().ok()?;
            (start == "0" && end.parse::<usize>().ok()?.checked_add(1) == Some(total) && total > 0)
                .then_some(total)
        });
    if status.is_success()
        && ((status != StatusCode::OK && status != StatusCode::PARTIAL_CONTENT)
            || ((range.is_some() || status == StatusCode::PARTIAL_CONTENT)
                && complete_length.is_none())
            || response.headers().get_all("content-range").iter().count() > 1)
    {
        return Err(FabioError::new(
            ErrorCode::ApiError,
            "Partial or unexpected OneLake metadata acknowledgement is not complete evidence",
        )
        .into());
    }
    if complete_length.is_some_and(|length| length > limit) {
        return Err(oversized().into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        FabioError::new(
            ErrorCode::NetworkError,
            "OneLake metadata body could not be read",
        )
    })? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(oversized().into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(error_from_status_body(
            status.as_u16(),
            &String::from_utf8_lossy(&bytes),
            None,
        )
        .into());
    }
    if complete_length.is_some_and(|length| length != bytes.len()) {
        return Err(FabioError::new(
            ErrorCode::ApiError,
            "OneLake metadata body did not cover its complete declared range",
        )
        .into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn body_limit_never_returns_a_truncated_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("metadata"))
            .mount(&server)
            .await;
        assert_eq!(
            bounded_bytes(reqwest::get(server.uri()).await.unwrap(), 8)
                .await
                .unwrap(),
            b"metadata"
        );
        assert!(
            bounded_bytes(reqwest::get(server.uri()).await.unwrap(), 7)
                .await
                .is_err()
        );
        assert!(
            bounded_bytes(reqwest::get(server.uri()).await.unwrap(), 0)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn failed_responses_are_not_successful_metadata_bytes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;
        let error = bounded_bytes(reqwest::get(server.uri()).await.unwrap(), 100)
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<FabioError>().unwrap().code,
            ErrorCode::Forbidden
        );
    }

    #[tokio::test]
    async fn partial_metadata_responses_are_never_complete_evidence() {
        for status in [200, 206] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .insert_header("content-range", "bytes 0-3/100")
                        .set_body_string("part"),
                )
                .mount(&server)
                .await;
            assert!(
                bounded_bytes(reqwest::get(server.uri()).await.unwrap(), 100)
                    .await
                    .is_err()
            );
        }
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("content-range", "bytes 0-7/8")
                    .set_body_string("metadata"),
            )
            .mount(&server)
            .await;
        assert_eq!(
            bounded_bytes(reqwest::get(server.uri()).await.unwrap(), 100)
                .await
                .unwrap(),
            b"metadata"
        );
    }
}
