use crate::memory::{HStackWorld, WorkingMemory};
use crate::manager::ContextManager;
use crate::control::AgentControlSystem;
use crate::tool::Tool;
use crate::action::{AgentAction, WorkingMemoryDelta};
use crate::provider::{LlmProvider, Message};
use crate::error::Error;
use hstack_core::sync::SyncAction;
use tracing::{debug, info, warn};
use futures::future::BoxFuture;

/// The central orchestrator of the agentic harness.
/// Implements the functional loop: C_n+1 = f(C_n, x)(C_n)
pub struct Agent {
    pub provider: Box<dyn LlmProvider>,
    pub manager: Box<dyn ContextManager>,
    pub control: Box<dyn AgentControlSystem>,
    pub tools: Vec<Box<dyn Tool>>,
    pub base_prompt: String,
}

impl Agent {
    /// Runs the agentic loop until completion or max depth.
    /// Returns the final answer string and a list of validated SyncActions (the "Delta List").
    pub async fn run(
        &self,
        world: &dyn HStackWorld,
        memory: &mut WorkingMemory,
    ) -> Result<(String, Vec<SyncAction>), Error> {
        let max_iterations = 10;
        let mut iterations = 0;
        let mut collected_deltas = Vec::new();

        loop {
            if iterations >= max_iterations {
                return Err(Error::MaxIterations);
            }

            info!(iteration = iterations, "Starting agent reasoning step");

            // 1. Construct context (C_n)
            let messages = self.manager.construct_context(world, memory, &self.base_prompt).await?;

            // 2. Prepare tool schemas
            let tool_schemas: Vec<hstack_core::provider::Tool> = self.tools.iter().map(|t| {
                hstack_core::provider::Tool {
                    r#type: "function".to_string(),
                    function: hstack_core::provider::ToolFunction {
                        name: t.name().to_string(),
                        description: t.description().to_string(),
                        parameters: t.parameters(),
                    }
                }
            }).collect();

            // 3. Generate response from provider
            let response = self.provider.generate_content(&messages, Some(&tool_schemas)).await?;

            // 4. Resolve the response into actions
            let action = self.resolve_response_to_action(response.clone(), world).await?;

            // 5. Apply the transition and collect deltas
            match self.apply_action(action, world, memory, &mut collected_deltas).await? {
                Some(final_answer) => {
                    info!(deltas = collected_deltas.len(), "Agent reached terminal state");
                    return Ok((final_answer, collected_deltas));
                }
                None => {
                    iterations += 1;
                }
            }
        }
    }

    async fn resolve_response_to_action(&self, response: Message, world: &dyn HStackWorld) -> Result<AgentAction, Error> {
        let mut actions = Vec::new();

        if response.content.is_some() || response.tool_calls.is_some() {
            actions.push(AgentAction::UpdateWorkingMemory(WorkingMemoryDelta::AppendMessage(response.clone())));
        }

        if let Some(tool_calls) = response.tool_calls {
            for call in tool_calls {
                let tool = self.tools.iter().find(|t| t.name() == call.function.name);
                match tool {
                    Some(t) => {
                        let args = serde_json::from_str(&call.function.arguments).unwrap_or(serde_json::Value::Null);
                        debug!(tool = %t.name(), "Executing tool");
                        let tool_action = t.execute(args, world).await?;
                        actions.push(tool_action);
                    }
                    None => {
                        warn!(tool = %call.function.name, "LLM requested unknown tool");
                        actions.push(AgentAction::UpdateWorkingMemory(WorkingMemoryDelta::AddTechnicalNoise(
                            call.function.name.clone(),
                            serde_json::json!({ "error": "Unknown tool" })
                        )));
                    }
                }
            }
        }

        if actions.is_empty() {
             Ok(AgentAction::Stop("Agent stalled".to_string()))
        } else if actions.len() == 1 {
            Ok(actions.remove(0))
        } else {
            Ok(AgentAction::Compound(actions))
        }
    }

    /// Recursively applies an action to the state.
    /// Captures stack mutations in the `deltas` list for future syncing.
    pub fn apply_action<'a>(
        &'a self,
        action: AgentAction,
        world: &'a dyn HStackWorld,
        memory: &'a mut WorkingMemory,
        deltas: &'a mut Vec<SyncAction>,
    ) -> BoxFuture<'a, Result<Option<String>, Error>> {
        Box::pin(async move {
            match action {
                AgentAction::Stop(answer) => Ok(Some(answer)),
                AgentAction::UpdateWorkingMemory(delta) => {
                    match delta {
                        WorkingMemoryDelta::AppendMessage(msg) => memory.messages.push(msg),
                        WorkingMemoryDelta::AddTechnicalNoise(key, val) => {
                            memory.technical_noise.push(serde_json::json!({ key: val }));
                        }
                    }
                    Ok(None)
                }
                AgentAction::UpdateStack(sync_action) => {
                    // Safety gate: only collected if approved
                    self.control.validate_stack_action(&sync_action).await?;
                    deltas.push(sync_action);
                    Ok(None)
                }
                AgentAction::Compound(actions) => {
                    let mut last_stop = None;
                    for a in actions {
                        if let Some(stop) = self.apply_action(a, world, memory, deltas).await? {
                            last_stop = Some(stop);
                        }
                    }
                    Ok(last_stop)
                }
            }
        })
    }
}
