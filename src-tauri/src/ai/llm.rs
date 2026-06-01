//! Local LLM client for AI-powered filename suggestions.
//!
//! Supports two API formats:
//! - **Ollama** (`/api/generate`) — default, runs at `http://localhost:11434`
//! - **OpenAI-compatible** (`/v1/chat/completions`) — LM Studio, Jan, llama-server, etc.
//!
//! The correct format is selected by inspecting the endpoint URL path.
//! If the LLM is unreachable or times out the functions return `None`/`Err`
//! so the caller falls back to the rule-based renamer.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub endpoint: String,
    pub model:    String,
    pub timeout:  Duration,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:11434/api/generate".into(),
            model:    "llama3.2:1b".into(),
            timeout:  Duration::from_secs(10),
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Ask the local LLM to suggest a clean filename.
/// Returns `None` if the model is unreachable, times out, or returns garbage.
pub async fn suggest_filename(
    url:      &str,
    raw_name: &str,
    mime:     &str,
    config:   &LlmConfig,
) -> Option<String> {
    let mime_str = if mime.is_empty() { "unknown" } else { mime };
    let prompt = format!(
        "Suggest a clean filename for a downloaded file. \
Reply with ONLY the filename — no explanation, no quotes, no punctuation at the end.\n\n\
URL: {}\nOriginal filename: {}\nContent-type: {}\n\n\
Rules:\n\
- Preserve the original file extension\n\
- Title Case for movies / TV shows / music; lowercase-with-hyphens for software\n\
- Remove watermarks, duplicate-copy markers, CDN garbage\n\
- Concise: 2–6 words plus extension\n\n\
Filename:",
        url, raw_name, mime_str
    );

    let result = tokio::time::timeout(
        config.timeout,
        call_llm(&prompt, config),
    )
    .await;

    match result {
        Ok(Ok(text)) => {
            let cleaned = text
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .trim()
                .to_string();
            if cleaned.is_empty() || cleaned.len() > 200 || cleaned.contains('\n') {
                warn!("LLM returned unusable filename: {:?}", cleaned);
                None
            } else {
                debug!("LLM suggested filename: {:?}", cleaned);
                Some(cleaned)
            }
        }
        Ok(Err(e)) => {
            warn!("LLM request failed: {}", e);
            None
        }
        Err(_) => {
            warn!("LLM request timed out after {:?}", config.timeout);
            None
        }
    }
}

/// Verify the LLM is reachable.
/// Returns the model's response (should be something like "OK").
pub async fn test_connection(config: &LlmConfig) -> Result<String> {
    let response = tokio::time::timeout(
        Duration::from_secs(20),
        call_llm("Reply with exactly one word: OK", config),
    )
    .await
    .context("LLM connection timed out")??;

    Ok(response.trim().to_string())
}

// ── Internal dispatch ─────────────────────────────────────────────────────────

async fn call_llm(prompt: &str, config: &LlmConfig) -> Result<String> {
    let client = Client::builder()
        .timeout(config.timeout + Duration::from_secs(2)) // reqwest timeout slightly wider
        .build()?;

    if config.endpoint.contains("/v1/") || config.endpoint.contains("chat/completions") {
        call_openai(&client, prompt, config).await
    } else {
        call_ollama(&client, prompt, config).await
    }
}

// ── Ollama ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model:  &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

async fn call_ollama(client: &Client, prompt: &str, config: &LlmConfig) -> Result<String> {
    let body = OllamaRequest {
        model:  &config.model,
        prompt,
        stream: false,
    };

    let resp = client
        .post(&config.endpoint)
        .json(&body)
        .send()
        .await
        .context("Failed to reach Ollama endpoint")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text   = resp.text().await.unwrap_or_default();
        anyhow::bail!("Ollama returned HTTP {}: {}", status, text);
    }

    let parsed: OllamaResponse = resp.json().await.context("Failed to parse Ollama response")?;
    Ok(parsed.response)
}

// ── OpenAI-compatible ─────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model:      &'a str,
    messages:   Vec<OpenAiMessage<'a>>,
    max_tokens: u32,
    stream:     bool,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role:    &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiContentMsg,
}

#[derive(Deserialize)]
struct OpenAiContentMsg {
    content: String,
}

async fn call_openai(client: &Client, prompt: &str, config: &LlmConfig) -> Result<String> {
    let body = OpenAiRequest {
        model: &config.model,
        messages: vec![OpenAiMessage { role: "user", content: prompt }],
        max_tokens: 100,
        stream: false,
    };

    let resp = client
        .post(&config.endpoint)
        .json(&body)
        .send()
        .await
        .context("Failed to reach OpenAI-compatible endpoint")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text   = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI endpoint returned HTTP {}: {}", status, text);
    }

    let parsed: OpenAiResponse = resp.json().await.context("Failed to parse OpenAI response")?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| anyhow::anyhow!("OpenAI response contained no choices"))
}
