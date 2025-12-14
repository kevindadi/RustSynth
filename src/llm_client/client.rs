use thiserror::Error;

/// LLM 提供商
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    /// OpenAI GPT-4
    OpenAi,
    /// Anthropic Claude
    Claude,
    /// DeepSeek
    DeepSeek,
}

/// LLM 配置
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// API 提供商
    pub provider: LlmProvider,
    /// API Key
    pub api_key: String,
    /// API 基础 URL（可选，某些提供商可能需要自定义）
    pub base_url: Option<String>,
    /// 模型名称（如 "gpt-4", "claude-3-opus-20240229", "deepseek-chat"）
    pub model: String,
    /// 温度参数（0.0-2.0）
    pub temperature: f64,
    /// 最大 tokens
    pub max_tokens: Option<u32>,
}

/// LLM 客户端
pub struct LlmClient {
    config: LlmConfig,
    http_client: reqwest::Client,
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("HTTP 错误: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API 错误: {0}")]
    Api(String),
    #[error("解析错误: {0}")]
    Parse(String),
    #[error("配置错误: {0}")]
    Config(String),
}

impl LlmClient {
    /// 创建新的 LLM 客户端
    pub fn new(config: LlmConfig) -> Result<Self, LlmError> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        Ok(Self {
            config,
            http_client,
        })
    }

    pub async fn generate(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String, LlmError> {
        match self.config.provider {
            LlmProvider::OpenAi => self.call_openai(prompt, system_prompt).await,
            LlmProvider::Claude => self.call_claude(prompt, system_prompt).await,
            LlmProvider::DeepSeek => self.call_deepseek(prompt, system_prompt).await,
        }
    }

    async fn call_openai(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String, LlmError> {
        let base_url = self.config.base_url.as_deref()
            .unwrap_or("https://api.openai.com/v1/chat/completions");

        let mut messages = Vec::new();
        
        if let Some(system) = system_prompt {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system
            }));
        }

        messages.push(serde_json::json!({
            "role": "user",
            "content": prompt
        }));

        let payload = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens.unwrap_or(4000),
        });

        let response = self.http_client
            .post(base_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(LlmError::Api(format!("OpenAI API 错误: {}", error_text)));
        }

        let json: serde_json::Value = response.json().await?;
        
        json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LlmError::Parse("无法解析 OpenAI 响应".to_string()))
            .map(|s| s.to_string())
    }

    async fn call_claude(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String, LlmError> {
        let base_url = self.config.base_url.as_deref()
            .unwrap_or("https://api.anthropic.com/v1/messages");

        let messages = vec![serde_json::json!({
            "role": "user",
            "content": prompt
        })];

        let mut payload = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens.unwrap_or(4096),
        });

        if let Some(system) = system_prompt {
            payload["system"] = serde_json::Value::String(system.to_string());
        }

        let response = self.http_client
            .post(base_url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(LlmError::Api(format!("Claude API 错误: {}", error_text)));
        }

        let json: serde_json::Value = response.json().await?;
        
        json["content"][0]["text"]
            .as_str()
            .ok_or_else(|| LlmError::Parse("无法解析 Claude 响应".to_string()))
            .map(|s| s.to_string())
    }

    async fn call_deepseek(&self, prompt: &str, system_prompt: Option<&str>) -> Result<String, LlmError> {
        let base_url = self.config.base_url.as_deref()
            .unwrap_or("https://api.deepseek.com/v1/chat/completions");

        let mut messages = Vec::new();
        
        if let Some(system) = system_prompt {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system
            }));
        }

        messages.push(serde_json::json!({
            "role": "user",
            "content": prompt
        }));

        let payload = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens.unwrap_or(4000),
        });

        let response = self.http_client
            .post(base_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(LlmError::Api(format!("DeepSeek API 错误: {}", error_text)));
        }

        let json: serde_json::Value = response.json().await?;
        
        json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LlmError::Parse("无法解析 DeepSeek 响应".to_string()))
            .map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_openai_client() {
        let config = LlmConfig {
            provider: LlmProvider::OpenAi,
            api_key: "test-key".to_string(),
            base_url: None,
            model: "gpt-4".to_string(),
            temperature: 0.7,
            max_tokens: Some(1000),
        };
    }
}
