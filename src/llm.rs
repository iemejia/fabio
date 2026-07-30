//! Minimal `OpenAI`-compatible LLM client used to power agent-quality features
//! that require an external judge model — few-shot validation and evaluation
//! grading. Fabio itself hosts no model; the caller supplies an endpoint + key
//! + model (deployment) via flags or `FABIO_LLM_*` env vars.
//!
//! Two endpoint flavors are supported, auto-detected from the endpoint host:
//! - **Azure `OpenAI` / Azure AI Foundry** (`*.azure.com`): the chat URL is
//!   `{endpoint}/openai/deployments/{model}/chat/completions?api-version=...`
//!   authenticated with the `api-key` header.
//! - **`OpenAI`-compatible** (anything else, e.g. `https://api.openai.com/v1`):
//!   the chat URL is `{endpoint}/chat/completions`, authenticated with
//!   `Authorization: Bearer {key}`, and the model goes in the request body.

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;

use crate::errors::{ErrorCode, FabioError};

/// Default Azure `OpenAI` REST API version (stable, widely available).
pub const DEFAULT_LLM_API_VERSION: &str = "2024-10-21";

/// Raw LLM configuration, typically sourced from CLI flags (with `FABIO_LLM_*`
/// env fallbacks wired via clap `env=`).
#[derive(Debug, Default, Clone)]
pub struct LlmConfig {
    pub endpoint: Option<String>,
    pub key: Option<String>,
    pub model: Option<String>,
    pub api_version: Option<String>,
}

impl LlmConfig {
    /// True when all three required fields (endpoint, key, model) are present.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        non_empty(self.endpoint.as_deref()).is_some()
            && non_empty(self.key.as_deref()).is_some()
            && non_empty(self.model.as_deref()).is_some()
    }
}

/// A configured LLM client ready to issue chat completions.
pub struct LlmClient {
    endpoint: String,
    key: String,
    model: String,
    api_version: String,
    azure: bool,
}

impl LlmClient {
    /// Build a client from configuration, validating that all required fields
    /// are present. Returns an actionable error listing the missing flags.
    pub fn from_config(cfg: &LlmConfig) -> Result<Self> {
        let endpoint = non_empty(cfg.endpoint.as_deref()).ok_or_else(missing_llm_error)?;
        let key = non_empty(cfg.key.as_deref()).ok_or_else(missing_llm_error)?;
        let model = non_empty(cfg.model.as_deref()).ok_or_else(missing_llm_error)?;
        // fabio only talks to remote endpoints over TLS — reject a plaintext
        // endpoint (except loopback) so the LLM API key is never sent in the
        // clear. Loopback http is allowed for locally-hosted model servers.
        if !crate::client::is_secure_or_loopback(endpoint) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("--llm-endpoint must be an https:// URL (got: {endpoint})"),
                "fabio only communicates with remote LLM endpoints over HTTPS so the API key is \
                 not sent in plaintext. Use the https:// endpoint of your Azure OpenAI / \
                 OpenAI-compatible resource (plaintext http:// is allowed only for \
                 loopback/localhost model servers).",
            )
            .into());
        }
        let api_version = non_empty(cfg.api_version.as_deref())
            .unwrap_or(DEFAULT_LLM_API_VERSION)
            .to_string();
        Ok(Self {
            azure: is_azure_endpoint(endpoint),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            key: key.to_string(),
            model: model.to_string(),
            api_version,
        })
    }

    /// The fully-qualified chat-completions URL for this client.
    fn chat_url(&self) -> String {
        build_chat_url(&self.endpoint, &self.model, &self.api_version, self.azure)
    }

    /// Send a system+user prompt and return the assistant's text content.
    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let http = crate::client::http_client_builder()
            .timeout(Duration::from_mins(3))
            .build()
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        let body = build_chat_body(&self.model, system, user, self.azure);
        let mut req = http
            .post(self.chat_url())
            .header("Content-Type", "application/json");
        req = if self.azure {
            req.header("api-key", &self.key)
        } else {
            req.header("Authorization", format!("Bearer {}", self.key))
        };

        let resp = req.json(&body).send().await.map_err(|e| {
            FabioError::with_hint(
                ErrorCode::NetworkError,
                format!("LLM request failed: {e}"),
                "Verify --llm-endpoint is reachable and correct (Azure OpenAI resource endpoint, \
                 e.g. https://<name>.openai.azure.com, or an OpenAI-compatible base URL).",
            )
        })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let truncated: String = text.chars().take(400).collect();
            return Err(FabioError::with_hint(
                ErrorCode::ApiError,
                format!("LLM endpoint returned HTTP {status}: {truncated}"),
                "Check --llm-key, --llm-model (the Azure *deployment* name, not the base model), \
                 and --llm-api-version.",
            )
            .into());
        }

        let json: Value = resp.json().await.map_err(|e| {
            FabioError::new(ErrorCode::ApiError, format!("Parse LLM response: {e}"))
        })?;
        extract_content(&json).ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "LLM response contained no message content".to_string(),
            )
            .into()
        })
    }

    /// Send a prompt and parse the assistant's reply as JSON (lenient: tolerates
    /// Markdown code fences and surrounding prose).
    pub async fn complete_json(&self, system: &str, user: &str) -> Result<Value> {
        let text = self.complete(system, user).await?;
        extract_json(&text).ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::ApiError,
                "LLM did not return valid JSON".to_string(),
                "Try a more capable --llm-model. The raw reply was not parseable as JSON.",
            )
            .into()
        })
    }
}

/// Return the trimmed string if it is non-empty.
fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn missing_llm_error() -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::InvalidInput,
        "LLM endpoint, key, and model are all required for this feature".to_string(),
        "Provide --llm-endpoint, --llm-key, and --llm-model (or set FABIO_LLM_ENDPOINT, \
         FABIO_LLM_KEY, FABIO_LLM_MODEL). For Azure OpenAI, --llm-model is the deployment name.",
    )
    .into()
}

/// Heuristic: an Azure `OpenAI` / Foundry endpoint lives under `*.azure.com`
/// (covers `openai.azure.com`, `cognitiveservices.azure.com`,
/// `services.ai.azure.com`). Everything else is treated as `OpenAI`-compatible.
fn is_azure_endpoint(endpoint: &str) -> bool {
    endpoint.to_ascii_lowercase().contains(".azure.com")
}

/// Build the chat-completions URL for the given flavor. Pure for testing.
fn build_chat_url(endpoint: &str, model: &str, api_version: &str, azure: bool) -> String {
    let base = endpoint.trim_end_matches('/');
    if azure {
        format!("{base}/openai/deployments/{model}/chat/completions?api-version={api_version}")
    } else {
        format!("{base}/chat/completions")
    }
}

/// Build the chat-completions request body. The model field is only needed for
/// the `OpenAI`-compatible flavor (Azure encodes it in the URL). Pure for testing.
fn build_chat_body(model: &str, system: &str, user: &str, azure: bool) -> Value {
    let mut body = serde_json::json!({
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    });
    if !azure {
        body["model"] = Value::from(model);
    }
    body
}

/// Extract `choices[0].message.content` from a chat-completions response.
fn extract_content(json: &Value) -> Option<String> {
    json.get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Leniently parse JSON out of an LLM reply: strips Markdown code fences and,
/// failing a direct parse, extracts the first balanced `{...}` or `[...]` span.
/// Pure.
fn extract_json(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    // Strip a fenced code block if present.
    let unfenced = strip_code_fence(trimmed);
    if let Ok(v) = serde_json::from_str::<Value>(unfenced) {
        return Some(v);
    }
    // Fall back to the first balanced object/array span.
    let span = first_json_span(unfenced)?;
    serde_json::from_str::<Value>(span).ok()
}

/// Remove a surrounding Markdown code fence (```json ... ``` or ``` ... ```).
fn strip_code_fence(text: &str) -> &str {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // Drop an optional language tag on the first line.
        let after_lang = rest.split_once('\n').map_or(rest, |(_, body)| body);
        return after_lang.trim().trim_end_matches('`').trim();
    }
    t
}

/// Return the first balanced `{...}` or `[...]` substring, if any.
fn first_json_span(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let (open, close) = bytes.iter().enumerate().find_map(|(i, &b)| match b {
        b'{' => Some((i, b'}')),
        b'[' => Some((i, b']')),
        _ => None,
    })?;
    let open_ch = bytes[open];
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            x if x == open_ch => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return text.get(open..=i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_azure_endpoint_detects_azure_hosts() {
        assert!(is_azure_endpoint("https://foo.openai.azure.com"));
        assert!(is_azure_endpoint(
            "https://bar.cognitiveservices.azure.com/"
        ));
        assert!(is_azure_endpoint("https://baz.services.ai.azure.com"));
        assert!(!is_azure_endpoint("https://api.openai.com/v1"));
        assert!(!is_azure_endpoint("http://localhost:1234/v1"));
    }

    #[test]
    fn build_chat_url_azure() {
        let url = build_chat_url("https://r.openai.azure.com/", "gpt-4o", "2024-10-21", true);
        assert_eq!(
            url,
            "https://r.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21"
        );
    }

    #[test]
    fn build_chat_url_openai_compatible() {
        let url = build_chat_url("https://api.openai.com/v1", "gpt-4o", "unused", false);
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn build_chat_body_openai_includes_model() {
        let body = build_chat_body("gpt-4o", "sys", "usr", false);
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "usr");
    }

    #[test]
    fn build_chat_body_azure_omits_model() {
        let body = build_chat_body("dep", "sys", "usr", true);
        assert!(body.get("model").is_none());
    }

    #[test]
    fn extract_content_reads_choice() {
        let json = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hello"}}]
        });
        assert_eq!(extract_content(&json).as_deref(), Some("hello"));
    }

    #[test]
    fn extract_json_plain() {
        let v = extract_json(r#"{"a": 1}"#).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn extract_json_fenced() {
        let v = extract_json("```json\n{\"a\": 2}\n```").unwrap();
        assert_eq!(v["a"], 2);
    }

    #[test]
    fn extract_json_fenced_no_lang() {
        let v = extract_json("```\n[1, 2, 3]\n```").unwrap();
        assert_eq!(v[2], 3);
    }

    #[test]
    fn extract_json_with_surrounding_prose() {
        let v =
            extract_json("Sure! Here is the result:\n{\"ok\": true}\nHope that helps.").unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn extract_json_ignores_braces_in_strings() {
        let v = extract_json(r#"{"note": "a } brace in a string", "n": 5}"#).unwrap();
        assert_eq!(v["n"], 5);
        assert_eq!(v["note"], "a } brace in a string");
    }

    #[test]
    fn extract_json_none_when_absent() {
        assert!(extract_json("no json here").is_none());
    }

    #[test]
    fn config_is_configured() {
        let mut cfg = LlmConfig::default();
        assert!(!cfg.is_configured());
        cfg.endpoint = Some("https://x.openai.azure.com".to_string());
        cfg.key = Some("k".to_string());
        assert!(!cfg.is_configured());
        cfg.model = Some("gpt-4o".to_string());
        assert!(cfg.is_configured());
        cfg.model = Some("  ".to_string());
        assert!(!cfg.is_configured());
    }

    #[test]
    fn from_config_requires_all_fields() {
        assert!(LlmClient::from_config(&LlmConfig::default()).is_err());
        let cfg = LlmConfig {
            endpoint: Some("https://x.openai.azure.com".to_string()),
            key: Some("k".to_string()),
            model: Some("dep".to_string()),
            api_version: None,
        };
        let client = LlmClient::from_config(&cfg).unwrap();
        assert!(client.azure);
        assert_eq!(client.api_version, DEFAULT_LLM_API_VERSION);
    }

    #[test]
    fn from_config_rejects_non_https_endpoint() {
        // A remote (non-loopback) http endpoint must be rejected.
        let cfg = LlmConfig {
            endpoint: Some("http://api.evil.example.com/v1".to_string()),
            key: Some("k".to_string()),
            model: Some("m".to_string()),
            api_version: None,
        };
        let Err(e) = LlmClient::from_config(&cfg) else {
            panic!("expected remote http endpoint to be rejected");
        };
        assert!(e.to_string().contains("https://"), "got: {e}");
    }

    #[test]
    fn from_config_allows_loopback_http_for_local_models() {
        // Locally-hosted OpenAI-compatible servers (Ollama, LM Studio) are OK.
        let cfg = LlmConfig {
            endpoint: Some("http://localhost:11434/v1".to_string()),
            key: Some("k".to_string()),
            model: Some("m".to_string()),
            api_version: None,
        };
        assert!(LlmClient::from_config(&cfg).is_ok());
    }
}
