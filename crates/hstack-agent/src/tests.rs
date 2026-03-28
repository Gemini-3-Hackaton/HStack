#[cfg(test)]
mod tests {
    use crate::agent::Agent;
    use crate::memory::{InMemoryWorld, WorkingMemory};
    use crate::manager::SimpleContextManager;
    use crate::control::AllowAllControl;
    use crate::provider::{LlmProvider, Message, Role};
    use crate::tool::IdentityTool;
    use crate::action::{AgentAction, WorkingMemoryDelta};
    use crate::error::Error;
    use hstack_core::sync::{SyncAction, SyncActionType};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    struct MockProvider {
        pub responses: Arc<Mutex<Vec<Message>>>,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate_content(
            &self,
            _messages: &[Message],
            _tools: Option<&[crate::provider::Tool]>,
        ) -> Result<Message, Error> {
            let mut resps = self.responses.lock().unwrap();
            if resps.is_empty() {
                return Err(Error::Provider("No more mock responses".to_string()));
            }
            Ok(resps.remove(0))
        }
    }

    #[tokio::test]
    async fn test_agent_run_completion() {
        let world = InMemoryWorld { tickets: Vec::new() };
        let mut memory = WorkingMemory::new();
        
        let mock_response = Message {
            role: Role::Assistant,
            content: Some("I have finished the task.".to_string()),
            tool_calls: Some(vec![crate::provider::ToolCall {
                id: "call_123".to_string(),
                r#type: "function".to_string(),
                function: crate::provider::ToolFunctionCall {
                    name: "identity".to_string(),
                    arguments: r#"{"answer": "Task complete!"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
            name: None,
        };

        let agent = Agent {
            provider: Box::new(MockProvider { 
                responses: Arc::new(Mutex::new(vec![mock_response])) 
            }),
            manager: Box::new(SimpleContextManager),
            control: Box::new(AllowAllControl),
            tools: vec![Box::new(IdentityTool)],
            base_prompt: "You are a helpful assistant.".to_string(),
        };

        let (answer, deltas) = agent.run(&world, &mut memory).await.unwrap();
        
        assert_eq!(answer, "Task complete!");
        assert!(deltas.is_empty());
    }

    #[tokio::test]
    async fn test_agent_multi_turn_search() {
        let world = InMemoryWorld { tickets: Vec::new() };
        let mut memory = WorkingMemory::new();

        let resp1 = Message {
            role: Role::Assistant,
            content: Some("Searching...".to_string()),
            tool_calls: Some(vec![crate::provider::ToolCall {
                id: "call_search".to_string(),
                r#type: "function".to_string(),
                function: crate::provider::ToolFunctionCall {
                    name: "search_stack".to_string(),
                    arguments: r#"{"query": "test"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
            name: None,
        };

        let resp2 = Message {
            role: Role::Assistant,
            content: Some("Found it.".to_string()),
            tool_calls: Some(vec![crate::provider::ToolCall {
                id: "call_id".to_string(),
                r#type: "function".to_string(),
                function: crate::provider::ToolFunctionCall {
                    name: "identity".to_string(),
                    arguments: r#"{"answer": "Finished after search"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
            name: None,
        };

        let agent = Agent {
            provider: Box::new(MockProvider { 
                responses: Arc::new(Mutex::new(vec![resp1, resp2])) 
            }),
            manager: Box::new(SimpleContextManager),
            control: Box::new(AllowAllControl),
            tools: vec![Box::new(IdentityTool), Box::new(crate::tool::SearchStack)],
            base_prompt: "You are a helpful assistant.".to_string(),
        };

        let (answer, _) = agent.run(&world, &mut memory).await.unwrap();
        
        assert_eq!(answer, "Finished after search");
        assert!(memory.technical_noise.iter().any(|n| n.get("search_stack:test").is_some()));
    }

    #[tokio::test]
    async fn test_agent_delta_collection() {
        let world = InMemoryWorld { tickets: Vec::new() };
        let mut memory = WorkingMemory::new();

        // Simulate a "Compound" action that updates stack and then stops
        // In a real scenario, this might come from a "CreateTicket" tool
        // But for testing the reasoning loop's collection logic, we'll manually wrap it in a mock response call
        
        let sync_action = SyncAction {
            action_id: "act_1".to_string(),
            r#type: SyncActionType::Create,
            entity_id: "t1".to_string(),
            entity_type: "TASK".to_string(),
            status: None,
            payload: None,
            notes: None,
            timestamp: "now".to_string(),
        };

        // We wrap the UpdateStack action in a Compound action manually in Agent logic if tools return it
        // Or simple tool execution
        
        struct MutationTool {
            pub action: SyncAction,
        }
        #[async_trait]
        impl crate::tool::Tool for MutationTool {
            fn name(&self) -> &str { "mutate" }
            fn description(&self) -> &str { "mutates stack" }
            fn parameters(&self) -> serde_json::Value { serde_json::json!({}) }
            async fn execute(&self, _args: serde_json::Value, _world: &dyn crate::memory::HStackWorld) -> Result<AgentAction, Error> {
                Ok(AgentAction::UpdateStack(self.action.clone()))
            }
        }

        let resp1 = Message {
            role: Role::Assistant,
            content: Some("Mutating...".to_string()),
            tool_calls: Some(vec![crate::provider::ToolCall {
                id: "call_mut".to_string(),
                r#type: "function".to_string(),
                function: crate::provider::ToolFunctionCall {
                    name: "mutate".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            tool_call_id: None,
            name: None,
        };

        let resp2 = Message {
            role: Role::Assistant,
            content: Some("Stopping...".to_string()),
            tool_calls: Some(vec![crate::provider::ToolCall {
                id: "call_stop".to_string(),
                r#type: "function".to_string(),
                function: crate::provider::ToolFunctionCall {
                    name: "identity".to_string(),
                    arguments: r#"{"answer": "Done mutating"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
            name: None,
        };

        let agent = Agent {
            provider: Box::new(MockProvider { 
                responses: Arc::new(Mutex::new(vec![resp1, resp2])) 
            }),
            manager: Box::new(SimpleContextManager),
            control: Box::new(AllowAllControl),
            tools: vec![
                Box::new(IdentityTool), 
                Box::new(MutationTool { action: sync_action.clone() })
            ],
            base_prompt: "You are a helpful assistant.".to_string(),
        };

        let (answer, deltas) = agent.run(&world, &mut memory).await.unwrap();
        
        assert_eq!(answer, "Done mutating");
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].action_id, "act_1");
    }

    #[tokio::test]
    async fn test_local_rate_limiter_rps_shaping() {
        use crate::rate_limiter::{LocalRateLimiter, RateLimitConfig, RateLimiter};
        let config = RateLimitConfig {
            requests_per_second: 1,
            requests_per_minute: 60,
            tokens_per_minute: 1000,
        };
        let limiter = LocalRateLimiter::new();
        let provider = "test_provider";

        // First request should be instant
        let start = std::time::Instant::now();
        limiter.acquire(provider, 1, 0, &config).await.unwrap();
        assert!(start.elapsed().as_millis() < 50);

        // Second request should have ~1s wait
        let start = std::time::Instant::now();
        limiter.acquire(provider, 1, 0, &config).await.unwrap();
        let elapsed = start.elapsed().as_millis();
        assert!(elapsed >= 1000, "Expected at least 1s wait, got {}ms", elapsed);
        assert!(elapsed < 1200); // 1s + jitter
    }

    #[tokio::test]
    async fn test_local_rate_limiter_tpm_shaping() {
        use crate::rate_limiter::{LocalRateLimiter, RateLimitConfig, RateLimiter};
        let limiter = LocalRateLimiter::new();
        let provider = "test_provider_fast";

        let config_fast = RateLimitConfig {
            requests_per_second: 100,
            requests_per_minute: 1000,
            tokens_per_minute: 60000, // 1000 tokens per second
        };
        
        // Use 1000 tokens. Should be instant.
        limiter.acquire(provider, 1, 1000, &config_fast).await.unwrap();
        
        // Second 1000 tokens should wait ~1s
        let start = std::time::Instant::now();
        limiter.acquire(provider, 1, 1000, &config_fast).await.unwrap();
        let elapsed = start.elapsed().as_millis();
        assert!(elapsed >= 1000, "Expected ~1s wait for tokens, got {}ms", elapsed);
    }

    #[tokio::test]
    async fn test_local_rate_limiter_max_delay() {
        use crate::rate_limiter::{LocalRateLimiter, RateLimitConfig, RateLimiter};
        use crate::error::Error;
        let config = RateLimitConfig {
            requests_per_second: 1, // 1 req/s
            requests_per_minute: 60,
            tokens_per_minute: 1000,
        };
        let limiter = LocalRateLimiter::new();
        
        // Manually manipulate the state to simulate a deep queue (74 minutes)
        {
            let mut state = limiter.state.lock().await;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
            state.insert("rl:prov:greedy:batch:rps".to_string(), now + 4440.0);
        }

        let result = limiter.acquire("greedy", 1, 0, &config).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::RateLimitExceeded { wait_time } => assert!(wait_time > 4400.0),
            _ => panic!("Expected RateLimitExceeded error"),
        }
    }
}
