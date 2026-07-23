//! LLM client abstraction for MemPalace.
//!
//! A provider-agnostic async trait with stub implementations for OpenAI,
//! Anthropic, Ollama and a deterministic `MockClient` for testing. No API
//! keys are required at compile time; keys are resolved at runtime from the
//! constructor argument or the conventional environment variable.

use async_trait::async_trait;
use std::env;
use std::fmt;
use std::sync::Mutex;

/// Errors returned by LLM providers.
#[derive(Debug)]
pub struct LlmError(String);

impl LlmError {
    pub fn new<S: Into<String>>(msg: S) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for LlmError {}

impl From<String> for LlmError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for LlmError {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Result type used by the LLM client module.
pub type Result<T> = std::result::Result<T, LlmError>;

/// Provider-agnostic LLM client interface.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Generate a completion for the given prompt.
    async fn complete(&self, prompt: &str) -> Result<String>;
}

/// OpenAI-compatible client stub.
///
/// API key is read from the constructor argument or the `OPENAI_API_KEY`
/// environment variable. If neither is available, `complete` returns an error.
pub struct OpenAiClient {
    pub model: String,
    pub endpoint: String,
    api_key: Option<String>,
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl OpenAiClient {
    /// Create a new OpenAI client stub.
    pub fn new(model: impl Into<String>, endpoint: Option<&str>, api_key: Option<&str>) -> Self {
        let api_key = api_key
            .map(|s| s.to_string())
            .or_else(|| env::var("OPENAI_API_KEY").ok());
        let endpoint = endpoint
            .map(|s| s.to_string())
            .unwrap_or_else(|| "https://api.openai.com".to_string());
        Self {
            model: model.into(),
            endpoint,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Resolve the chat-completions URL from the configured endpoint.
    fn resolve_url(&self) -> String {
        let mut url = self.endpoint.trim_end_matches('/').to_string();
        if url.ends_with("/chat/completions") {
            return url;
        }
        if !url.ends_with("/v1") {
            url = format!("{}/v1", url);
        }
        format!("{}/chat/completions", url)
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn complete(&self, prompt: &str) -> Result<String> {
        if self.api_key.is_none() {
            return Err(LlmError::from(
                "OPENAI_API_KEY not configured (use env or constructor argument)",
            ));
        }
        let url = self.resolve_url();
        Ok(format!(
            "OpenAI stub completion for model '{}' at {}: prompt length {}",
            self.model,
            url,
            prompt.len()
        ))
    }
}

/// Anthropic Messages API client stub.
///
/// API key is read from the constructor argument or the `ANTHROPIC_API_KEY`
/// environment variable.
pub struct AnthropicClient {
    pub model: String,
    pub endpoint: String,
    api_key: Option<String>,
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl AnthropicClient {
    pub fn new(model: impl Into<String>, endpoint: Option<&str>, api_key: Option<&str>) -> Self {
        let api_key = api_key
            .map(|s| s.to_string())
            .or_else(|| env::var("ANTHROPIC_API_KEY").ok());
        let endpoint = endpoint
            .map(|s| s.to_string())
            .unwrap_or_else(|| "https://api.anthropic.com".to_string());
        Self {
            model: model.into(),
            endpoint,
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(&self, prompt: &str) -> Result<String> {
        if self.api_key.is_none() {
            return Err(LlmError::from(
                "ANTHROPIC_API_KEY not configured (use env or constructor argument)",
            ));
        }
        Ok(format!(
            "Anthropic stub completion for model '{}' at {}: prompt length {}",
            self.model,
            self.endpoint,
            prompt.len()
        ))
    }
}

/// Local Ollama client stub.
///
/// No API key is required; this provider is always local-first.
pub struct OllamaClient {
    pub model: String,
    pub endpoint: String,
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl OllamaClient {
    pub fn new(model: impl Into<String>, endpoint: Option<&str>) -> Self {
        let endpoint = endpoint
            .map(|s| s.to_string())
            .unwrap_or_else(|| "http://localhost:11434".to_string());
        Self {
            model: model.into(),
            endpoint,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn complete(&self, prompt: &str) -> Result<String> {
        Ok(format!(
            "Ollama stub completion for model '{}' at {}: prompt length {}",
            self.model,
            self.endpoint,
            prompt.len()
        ))
    }
}

/// Deterministic mock client for unit tests.
pub struct MockClient {
    response: String,
    calls: Mutex<Vec<String>>,
}

impl MockClient {
    /// Create a mock client that always returns `response`.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Return the prompts received so far.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    /// Return the number of prompts received.
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait]
impl LlmClient for MockClient {
    async fn complete(&self, prompt: &str) -> Result<String> {
        self.calls.lock().unwrap().push(prompt.to_string());
        Ok(self.response.clone())
    }
}

/// Factory for constructing a boxed LLM client by provider name.
///
/// Supported providers:
/// - `openai` / `openai-compat`
/// - `anthropic`
/// - `ollama`
/// - `mock`
///
/// If `model` is `None`, a sensible default is chosen for the provider.
pub fn create_client(provider: &str, model: Option<&str>) -> Result<Box<dyn LlmClient>> {
    match provider.to_ascii_lowercase().as_str() {
        "openai" | "openai-compat" => Ok(Box::new(OpenAiClient::new(
            model.unwrap_or("gpt-4o-mini"),
            None,
            None,
        ))),
        "anthropic" => Ok(Box::new(AnthropicClient::new(
            model.unwrap_or("claude-3-haiku-20240307"),
            None,
            None,
        ))),
        "ollama" => Ok(Box::new(OllamaClient::new(
            model.unwrap_or("llama3.1"),
            None,
        ))),
        "mock" => Ok(Box::new(MockClient::new(model.unwrap_or("mock response")))),
        _ => Err(LlmError::from(format!(
            "Unknown provider '{}'. Choices: openai, anthropic, ollama, mock",
            provider
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn remove_env(key: &str) -> Option<String> {
        let prev = env::var(key).ok();
        env::remove_var(key);
        prev
    }

    fn restore_env(key: &str, value: Option<String>) {
        if let Some(v) = value {
            env::set_var(key, v);
        } else {
            env::remove_var(key);
        }
    }

    #[tokio::test]
    async fn llm_client_mock_records_prompts_and_returns_response() {
        let client = MockClient::new("mocked");
        assert_eq!(client.complete("hello").await.unwrap(), "mocked");
        assert_eq!(client.complete("world").await.unwrap(), "mocked");
        assert_eq!(
            client.calls(),
            vec!["hello".to_string(), "world".to_string()]
        );
        assert_eq!(client.call_count(), 2);
    }

    #[tokio::test]
    async fn llm_client_ollama_stub_succeeds_without_key() {
        let client = OllamaClient::new("qwen2.5", Some("http://localhost:11435"));
        let result = client.complete("local prompt").await.unwrap();
        assert!(result.contains("Ollama stub completion"));
        assert!(result.contains("qwen2.5"));
        assert!(result.contains("localhost:11435"));
        assert!(result.contains("12"));
    }

    #[tokio::test]
    async fn llm_client_ollama_default_endpoint() {
        let client = OllamaClient::new("llama3.1", None);
        assert_eq!(client.endpoint, "http://localhost:11434");
        let result = client.complete("x").await.unwrap();
        assert!(result.contains("localhost:11434"));
    }

    #[tokio::test]
    async fn llm_client_openai_stub_with_explicit_key() {
        let client = OpenAiClient::new("gpt-4o", Some("https://api.example.com"), Some("sk-test"));
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("OpenAI stub completion"));
        assert!(result.contains("gpt-4o"));
        assert!(result.contains("https://api.example.com/v1/chat/completions"));
        assert!(result.contains("6"));
    }

    #[tokio::test]
    async fn llm_client_openai_stub_url_already_has_v1() {
        let client =
            OpenAiClient::new("gpt-4", Some("https://api.example.com/v1"), Some("sk-test"));
        let result = client.complete("p").await.unwrap();
        assert!(result.contains("https://api.example.com/v1/chat/completions"));
        assert!(!result.contains("/v1/v1"));
    }

    #[tokio::test]
    async fn llm_client_openai_stub_url_already_has_chat_completions() {
        let client = OpenAiClient::new(
            "gpt-4",
            Some("https://api.example.com/v1/chat/completions"),
            Some("sk-test"),
        );
        let result = client.complete("p").await.unwrap();
        assert!(result.contains("https://api.example.com/v1/chat/completions"));
        assert!(!result.contains("/chat/completions/chat/completions"));
    }

    #[tokio::test]
    async fn llm_client_openai_stub_without_key_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = remove_env("OPENAI_API_KEY");
        let client = OpenAiClient::new("gpt-4", None::<&str>, None::<&str>);
        let err = client.complete("prompt").await.unwrap_err();
        assert!(err.to_string().contains("OPENAI_API_KEY"));
        restore_env("OPENAI_API_KEY", prev);
    }

    #[tokio::test]
    async fn llm_client_openai_stub_reads_key_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = remove_env("OPENAI_API_KEY");
        env::set_var("OPENAI_API_KEY", "sk-env");
        let client = OpenAiClient::new("gpt-4", None::<&str>, None::<&str>);
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("OpenAI stub completion"));
        restore_env("OPENAI_API_KEY", prev);
    }

    #[tokio::test]
    async fn llm_client_anthropic_stub_with_explicit_key() {
        let client = AnthropicClient::new(
            "claude-3-opus",
            Some("https://api.anthropic.test"),
            Some("sk-ant-test"),
        );
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("Anthropic stub completion"));
        assert!(result.contains("claude-3-opus"));
        assert!(result.contains("https://api.anthropic.test"));
        assert!(result.contains("6"));
    }

    #[tokio::test]
    async fn llm_client_anthropic_stub_without_key_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = remove_env("ANTHROPIC_API_KEY");
        let client = AnthropicClient::new("claude-3", None::<&str>, None::<&str>);
        let err = client.complete("prompt").await.unwrap_err();
        assert!(err.to_string().contains("ANTHROPIC_API_KEY"));
        restore_env("ANTHROPIC_API_KEY", prev);
    }

    #[tokio::test]
    async fn llm_client_anthropic_stub_reads_key_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = remove_env("ANTHROPIC_API_KEY");
        env::set_var("ANTHROPIC_API_KEY", "sk-ant-env");
        let client = AnthropicClient::new("claude-3", None::<&str>, None::<&str>);
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("Anthropic stub completion"));
        restore_env("ANTHROPIC_API_KEY", prev);
    }

    #[tokio::test]
    async fn llm_client_create_client_mock() {
        let client = create_client("mock", None).unwrap();
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("mock response"));
    }

    #[tokio::test]
    async fn llm_client_create_client_ollama() {
        let client = create_client("ollama", Some("qwen2.5")).unwrap();
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("Ollama stub completion"));
        assert!(result.contains("qwen2.5"));
    }

    #[tokio::test]
    async fn llm_client_create_client_openai() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = remove_env("OPENAI_API_KEY");
        env::set_var("OPENAI_API_KEY", "sk-env");
        let client = create_client("openai", None).unwrap();
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("OpenAI stub completion"));
        assert!(result.contains("gpt-4o-mini"));
        restore_env("OPENAI_API_KEY", prev);
    }

    #[tokio::test]
    async fn llm_client_create_client_openai_compat() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = remove_env("OPENAI_API_KEY");
        env::set_var("OPENAI_API_KEY", "sk-env");
        let client = create_client("openai-compat", Some("custom-model")).unwrap();
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("custom-model"));
        restore_env("OPENAI_API_KEY", prev);
    }

    #[tokio::test]
    async fn llm_client_create_client_anthropic() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = remove_env("ANTHROPIC_API_KEY");
        env::set_var("ANTHROPIC_API_KEY", "sk-ant-env");
        let client = create_client("anthropic", None).unwrap();
        let result = client.complete("prompt").await.unwrap();
        assert!(result.contains("Anthropic stub completion"));
        assert!(result.contains("claude-3-haiku-20240307"));
        restore_env("ANTHROPIC_API_KEY", prev);
    }

    #[tokio::test]
    async fn llm_client_create_client_unknown_provider_errors() {
        let result = create_client("unknown", None);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Unknown provider"));
        assert!(err.to_string().contains("unknown"));
    }

    #[tokio::test]
    async fn llm_client_openai_default_endpoint() {
        let client = OpenAiClient::new("gpt-4", None::<&str>, Some("sk-test"));
        assert_eq!(client.endpoint, "https://api.openai.com");
    }

    #[tokio::test]
    async fn llm_client_anthropic_default_endpoint() {
        let client = AnthropicClient::new("claude-3", None::<&str>, Some("sk-ant-test"));
        assert_eq!(client.endpoint, "https://api.anthropic.com");
    }

    #[tokio::test]
    async fn llm_client_trait_object_dispatch() {
        let clients: Vec<Box<dyn LlmClient>> = vec![
            Box::new(MockClient::new("a")),
            Box::new(OllamaClient::new("m", None)),
            Box::new(OpenAiClient::new("m", None::<&str>, Some("k"))),
            Box::new(AnthropicClient::new("m", None::<&str>, Some("k"))),
        ];
        for client in clients {
            let result = client.complete("x").await.unwrap();
            assert!(!result.is_empty());
        }
    }
}
