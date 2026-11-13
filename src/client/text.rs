//! File-like Fabric responses retain metadata and use a strict text LRO path.

use std::time::Duration;

use anyhow::Result;
use reqwest::{Response, StatusCode};
use serde_json::Value;

use super::{
    FabricClient, LRO_POLL_INTERVAL, error_from_failed_response, failed_lro_error,
    retry_after_from_headers, submission,
};
use crate::errors::{ErrorCode, FabioError};

pub struct TextResponse {
    pub content: String,
    pub etag: Option<String>,
    pub content_disposition: Option<String>,
    pub content_type: Option<String>,
    pub truncated: bool,
}

impl FabricClient {
    pub async fn get_text_response(&self, path: &str) -> Result<TextResponse> {
        Self::text_response(self.get_raw_response(path).await?).await
    }

    async fn text_response(response: Response) -> Result<TextResponse> {
        if !response.status().is_success() || response.status() == StatusCode::ACCEPTED {
            return Err(error_from_failed_response(response).await.into());
        }
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(String::from)
        };
        let etag = header("ETag");
        let content_disposition = header("Content-Disposition");
        let content_type = header("Content-Type");
        let truncated =
            header("x-ms-log-truncated").is_some_and(|value| value.eq_ignore_ascii_case("true"));
        let bytes = response.bytes().await?;
        let content = String::from_utf8(bytes.to_vec())
            .map_err(|_| FabioError::api_error("Text response is not valid UTF-8"))?;
        Ok(TextResponse {
            content,
            etag,
            content_disposition,
            content_type,
            truncated,
        })
    }

    /// Formatting returns transformed text without saving it. The POST has no body.
    pub async fn post_empty_readonly_text_with_accept(
        &self,
        path: &str,
        accept: &str,
    ) -> Result<TextResponse> {
        let url = self.fabric_url(path);
        let send = async || {
            let token = self.require_auth().await?;
            self.http
                .post(&url)
                .header(reqwest::header::AUTHORIZATION, token)
                .header("Accept", accept)
                .header("Content-Length", "0")
                .send()
                .await
                .map_err(|error| {
                    anyhow::Error::from(FabioError::new(ErrorCode::NetworkError, error.to_string()))
                })
        };
        let mut response: Response = send().await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            response = send().await?;
        }
        if response.status() != StatusCode::ACCEPTED {
            return Self::text_response(response).await;
        }
        self.poll_text_lro(response, accept).await
    }

    async fn poll_text_lro(&self, response: Response, accept: &str) -> Result<TextResponse> {
        let poll_url = response
            .headers()
            .get("Location")
            .and_then(|value| value.to_str().ok())
            .map(String::from)
            .or_else(|| {
                response
                    .headers()
                    .get("x-ms-operation-id")
                    .and_then(|value| value.to_str().ok())
                    .map(|id| self.fabric_url(&format!("/operations/{}", urlencoding::encode(id))))
            })
            .ok_or_else(|| {
                FabioError::api_error(
                    "Long-running text response did not include Location or x-ms-operation-id",
                )
            })?;
        submission::validate_observation_url(&poll_url, &self.fabric_endpoint())?;
        let mut interval =
            retry_after_from_headers(response.headers()).unwrap_or(LRO_POLL_INTERVAL);
        let deadline = tokio::time::Instant::now() + self.lro_max_wait;
        let poll = async {
            loop {
                tokio::time::sleep(interval.min(Duration::from_mins(1))).await;
                let response = self.get_text_url(&poll_url, "application/json").await?;
                interval = retry_after_from_headers(response.headers()).unwrap_or(interval);
                if response.status() == StatusCode::ACCEPTED {
                    continue;
                }
                if !response.status().is_success() {
                    return Err(error_from_failed_response(response).await.into());
                }
                let location = response
                    .headers()
                    .get("Location")
                    .and_then(|value| value.to_str().ok())
                    .map(String::from);
                let body: Value = response.json().await?;
                let status = body
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if status.eq_ignore_ascii_case("Succeeded") {
                    let location = location.ok_or_else(|| FabioError::api_error("Completed long-running text response did not include a result location"))?;
                    submission::validate_observation_url(&location, &self.fabric_endpoint())?;
                    let response = self.get_text_url(&location, accept).await?;
                    return Self::text_response(response).await;
                }
                if ["Failed", "Canceled", "Cancelled"]
                    .iter()
                    .any(|value| status.eq_ignore_ascii_case(value))
                {
                    return Err(failed_lro_error(&body).into());
                }
                if !["Running", "NotStarted", "InProgress"]
                    .iter()
                    .any(|value| status.eq_ignore_ascii_case(value))
                {
                    return Err(FabioError::api_error(
                        "Unrecognized text operation status; completion is unverified",
                    )
                    .into());
                }
            }
        };
        tokio::time::timeout_at(deadline, poll).await.map_err(|_| {
            FabioError::new(
                ErrorCode::Timeout,
                "Text operation polling timed out; completion is unverified",
            )
        })?
    }

    async fn get_text_url(&self, url: &str, accept: &str) -> Result<Response> {
        submission::validate_observation_url(url, &self.fabric_endpoint())?;
        let send = async || {
            let token = self.require_auth().await?;
            self.http
                .get(url)
                .header(reqwest::header::AUTHORIZATION, token)
                .header("Accept", accept)
                .send()
                .await
                .map_err(anyhow::Error::from)
        };
        let mut response = send().await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            response = send().await?;
        }
        Ok(response)
    }
}
