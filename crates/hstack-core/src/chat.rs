use crate::error::Error;
use crate::provider::{
    gemini::generate_gemini_content, openai_compat::generate_openai_content, Message,
    ProviderConfig, ProviderKind, Role, Tool, ToolCall, ToolFunctionCall,
};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use tracing::{debug, warn};

pub type ToolExecutor = Box<
    dyn Fn(String, Value) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send>>
        + Send
        + Sync,
>;

pub type ContextRefreshFn = Box<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send>>
        + Send
        + Sync,
>;

// Helper to parse potential tool calls from a JSON value
fn extract_tool_calls_from_json(json_val: &Value, tool_calls: &mut Vec<ToolCall>) {
    if let Some(arr) = json_val.as_array() {
        for item in arr {
            extract_single_tool_call(item, tool_calls);
        }
    } else {
        extract_single_tool_call(json_val, tool_calls);
    }
}

fn extract_single_tool_call(json_val: &Value, tool_calls: &mut Vec<ToolCall>) {
    let name_opt = json_val.get("name").and_then(|v| v.as_str());
    if let Some(name) = name_opt {
        let final_args = json_val.get("arguments").cloned().unwrap_or(Value::Null);
        
        tool_calls.push(ToolCall {
            id: uuid::Uuid::new_v4().to_string(),
            r#type: "function".to_string(),
            function: ToolFunctionCall {
                name: name.to_string(),
                arguments: match final_args {
                    Value::String(s) => s.clone(),
                    other => serde_json::to_string(&other).unwrap_or_else(|_| "{}".to_string()),
                },
            },
        });
    }
}

pub async fn chat_loop(
    config: &ProviderConfig,
    messages: &mut Vec<Message>,
    tools: &[Tool],
    tool_executor: &ToolExecutor,
    context_refresh: Option<&ContextRefreshFn>,
) -> Result<Message, Error> {
    let max_iterations = 5;
    let mut iterations = 0;

    loop {
        if iterations >= max_iterations {
            return Err(Error::MaxIterations);
        }

        let response_result = match config.kind {
            ProviderKind::OpenAiCompatible => {
                generate_openai_content(config, messages, Some(tools)).await
            }
            ProviderKind::Gemini => generate_gemini_content(config, messages, Some(tools)).await,
        };

        let response = match response_result {
            Ok(msg) => msg,
            Err(e) => return Err(e),
        };

        // Check if there are tool calls natively
        let mut tool_calls = response.tool_calls.clone().unwrap_or_default();

        // Fallback: If no native tool calls, check if the LLM outputted raw JSON representing a tool call
        if tool_calls.is_empty() {
            if let Some(content) = &response.content {
                debug!("no native tool calls returned; checking assistant content for fallback JSON");
                
                // Try parsing the entire string as JSON first
                if let Ok(json_val) = serde_json::from_str::<Value>(content) {
                    extract_tool_calls_from_json(&json_val, &mut tool_calls);
                } else {
                    // Extract code blocks, particularly ```json blocks
                    let mut start_idx = 0;
                    while let Some(start) = content[start_idx..].find("```") {
                        let absolute_start = start_idx + start;
                        // Find the end of the opening ``` (e.g., ```json\n)
                        if let Some(newline_pos) = content[absolute_start..].find('\n') {
                            let content_start = absolute_start + newline_pos + 1;
                            if let Some(end) = content[content_start..].find("```") {
                                let json_str = &content[content_start..content_start + end];
                                if let Ok(json_val) = serde_json::from_str::<Value>(json_str) {
                                    extract_tool_calls_from_json(&json_val, &mut tool_calls);
                                }
                                start_idx = content_start + end + 3;
                            } else {
                                // If we don't find a closing ```, stop to prevent infinite loops
                                break;
                            }
                        } else {
                            // If we don't find a newline after ```, stop
                            break;
                        }
                    }
                    
                    // If still empty, try to find a raw json object { ... }
                    if tool_calls.is_empty() {
                        if let Some(start) = content.find('{') {
                            if let Some(end) = content.rfind('}') {
                                if end > start {
                                    let json_str = &content[start..end + 1];
                                    if let Ok(json_val) = serde_json::from_str::<Value>(json_str) {
                                        extract_tool_calls_from_json(&json_val, &mut tool_calls);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if tool_calls.is_empty() {
            messages.push(response.clone());
            return Ok(response);
        }

        // We have tool calls
        let mut resp_with_tools = response.clone();
        resp_with_tools.tool_calls = Some(tool_calls.clone());
        messages.push(resp_with_tools);

        for call in tool_calls {
            let args_result = serde_json::from_str::<Value>(&call.function.arguments);
            let args = match args_result {
                Ok(v) => v,
                Err(_) => Value::Null,
            };

            let tool_result = tool_executor(call.function.name.clone(), args).await;

            let content = match tool_result {
                Ok(s) => s,
                Err(e) => format!("Error executing tool: {:?}", e),
            };

            messages.push(Message {
                role: Role::Tool,
                content: Some(content),
                tool_calls: None,
                tool_call_id: Some(call.id.clone()),
                name: Some(call.function.name.clone()),
            });
        }

        // After executing tool calls, refresh system context if refresh function provided
        if let Some(refresh_fn) = context_refresh {
            // Check if there's a system message to replace
            if let Some(system_msg_idx) = messages.iter().position(|m| m.role == Role::System) {
                // Generate fresh context and replace
                match refresh_fn().await {
                    Ok(fresh_prompt) => {
                        messages[system_msg_idx] = Message {
                            role: Role::System,
                            content: Some(fresh_prompt),
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                        };
                        debug!("system context refreshed after tool execution");
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to refresh system context after tool execution");
                    }
                }
            }
        }

        iterations += 1;
    }
}