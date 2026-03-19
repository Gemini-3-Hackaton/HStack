use super::{Message, ProviderConfig, Tool, Role};
use crate::error::Error;
use serde::{Deserialize, Serialize};


#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OpenAiChatRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a [Tool]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
}

#[derive(Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

pub async fn generate_openai_content(
    config: &ProviderConfig,
    messages: &[Message],
    tools: Option<&[Tool]>,
) -> Result<Message, Error> {
    let client = reqwest::Client::new();
    let api_url = format!("{}/chat/completions", config.endpoint.trim_end_matches('/'));


    let mut mapped_messages = Vec::new();
    for m in messages {
        if let Some(ref c) = m.content {
            mapped_messages.push(OpenAiMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                },
                content: c,
            });
        }
    }

    let request = OpenAiChatRequest {
        model: &config.model_name,
        messages: mapped_messages,
        temperature: Some(0.7),
        tools,
        tool_choice: if tools.is_some() { Some("auto") } else { None },
    };

    let mut headers = reqwest::header::HeaderMap::new();
    
    println!("--- SENDING LOCAL LLM REQUEST TO: {} ---", api_url);
    println!("PAYLOAD: {:?}", serde_json::to_string(&request).unwrap_or_default());

    match reqwest::header::HeaderValue::from_static("application/json") {
        val => headers.insert("Content-Type", val),
    };

    match reqwest::header::HeaderValue::from_static("https://hstack.app") {
        val => headers.insert("HTTP-Referer", val),
    };
    
    match reqwest::header::HeaderValue::from_static("HStack") {
        val => headers.insert("X-Title", val),
    };

    if !config.api_key.is_empty() {
        let auth_str = if config.api_key.starts_with("Bearer ") {
            config.api_key.clone()
        } else {
            format!("Bearer {}", config.api_key)
        };
        match reqwest::header::HeaderValue::from_str(&auth_str) {
            Ok(val) => {
                headers.insert("Authorization", val);
            }
            Err(e) => {
                return Err(Error::Header(format!("Invalid API key format: {}", e)));
            }
        }
    }

    let response_result = client
        .post(&api_url)
        .headers(headers)
        .json(&request)
        .send()
        .await;

    let response = match response_result {
        Ok(res) => res,
        Err(e) => return Err(Error::Network(e.to_string())),
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = match response.text().await {
            Ok(t) => t,
            Err(_) => "Could not read error body".to_string(),
        };
        println!("--- HTTP ERROR {} FROM API ---", status);
        println!("BODY: {}", body);
        return Err(Error::Provider(format!("API error (status {}): {}", status, body)));
    }

    let response_data_result: Result<OpenAiChatResponse, reqwest::Error> = response.json().await;
    let response_data = match response_data_result {
        Ok(data) => data,
        Err(e) => {
            let _ = std::fs::OpenOptions::new().create(true).append(true).open("hstack_debug.log").map(|mut f| std::io::Write::write_fmt(&mut f, format_args!("JSON Error: {}\\n", e)));
            return Err(Error::Internal(format!("Failed to parse response: {}", e)));
        }
    };

    let mut choices = response_data.choices;
    if choices.is_empty() {
        return Err(Error::Provider("API returned empty choices".to_string()));
    }

    let msg = choices.remove(0).message;
    let _ = std::fs::OpenOptions::new().create(true).append(true).open("hstack_debug.log").map(|mut f| std::io::Write::write_fmt(&mut f, format_args!("LLM RAW MESSAGE: {:?}\\n", msg)));
    Ok(msg)
}
