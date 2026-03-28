pub mod gemini;
pub mod openai_compat;

pub use hstack_core::provider::{Message, Role, Tool, ToolCall, ToolFunctionCall, ProviderKind, ProviderConfig};
use async_trait::async_trait;
use crate::error::Error;
use crate::rate_limiter::RateLimiter;
use std::sync::Arc;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate_content(
        &self,
        messages: &[Message],
        tools: Option<&[Tool]>,
    ) -> Result<Message, Error>;
}

pub struct GeminiProvider {
    pub config: ProviderConfig,
    pub limiter: Option<Arc<dyn RateLimiter>>,
}

impl GeminiProvider {
    pub fn new(config: ProviderConfig, limiter: Option<Arc<dyn RateLimiter>>) -> Self {
        Self { config, limiter }
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn generate_content(
        &self,
        messages: &[Message],
        tools: Option<&[Tool]>,
    ) -> Result<Message, Error> {
        // Enforce rate limit if configured
        if let (Some(limiter), Some(limit_config)) = (&self.limiter, &self.config.rate_limit) {
            // Estimate costs (RPS=1, TPM=len/4)
            let text_total: String = messages.iter().filter_map(|m| m.content.clone()).collect();
            let token_cost = (text_total.len() / 4) as u32; // Simplified estimation
            
            limiter.acquire(&self.config.name, 1, token_cost, limit_config).await?;
        }
        
        gemini::generate_gemini_content(&self.config, messages, tools).await
    }
}

pub struct OpenAiProvider {
    pub config: ProviderConfig,
    pub limiter: Option<Arc<dyn RateLimiter>>,
}

impl OpenAiProvider {
    pub fn new(config: ProviderConfig, limiter: Option<Arc<dyn RateLimiter>>) -> Self {
        Self { config, limiter }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn generate_content(
        &self,
        messages: &[Message],
        tools: Option<&[Tool]>,
    ) -> Result<Message, Error> {
        // Enforce rate limit if configured
        if let (Some(limiter), Some(limit_config)) = (&self.limiter, &self.config.rate_limit) {
            let text_total: String = messages.iter().filter_map(|m| m.content.clone()).collect();
            let token_cost = (text_total.len() / 4) as u32;
            
            limiter.acquire(&self.config.name, 1, token_cost, limit_config).await?;
        }
        
        openai_compat::generate_openai_content(&self.config, messages, tools).await
    }
}
