use std::{env, process::ExitCode};

use lingxi_core::{
    lmp::{ContentBlock, LmpMessage, LmpRequest, Role, UnifiedClient},
    NativeProtocol,
};

fn setting(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn bedrock_key() -> Option<String> {
    if let Some(key) = setting("AWS_BEARER_TOKEN_BEDROCK") {
        return Some(key);
    }
    let access = setting("AWS_ACCESS_KEY_ID")?;
    let secret = setting("AWS_SECRET_ACCESS_KEY")?;
    Some(match setting("AWS_SESSION_TOKEN") {
        Some(token) => format!("{access}:{secret}:{token}"),
        None => format!("{access}:{secret}"),
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let bedrock = setting("AWS_REGION")
        .map(|region| format!("https://bedrock-runtime.{region}.amazonaws.com"));
    let vertex = setting("GOOGLE_CLOUD_PROJECT")
        .zip(setting("GOOGLE_CLOUD_LOCATION"))
        .map(|(project, location)| {
            let host = if location == "global" {
                "aiplatform.googleapis.com".to_owned()
            } else {
                format!("{location}-aiplatform.googleapis.com")
            };
            format!("https://{host}/v1/projects/{project}/locations/{location}/publishers/google/models")
        });
    let probes = [
        (
            NativeProtocol::OpenaiChatCompletions,
            Some("https://api.openai.com/v1".into()),
            "OPENAI_CHAT_MODEL",
            setting("OPENAI_API_KEY"),
        ),
        (
            NativeProtocol::OpenaiResponses,
            Some("https://api.openai.com/v1".into()),
            "OPENAI_RESPONSES_MODEL",
            setting("OPENAI_API_KEY"),
        ),
        (
            NativeProtocol::AnthropicMessages,
            Some("https://api.anthropic.com".into()),
            "ANTHROPIC_MODEL",
            setting("ANTHROPIC_API_KEY"),
        ),
        (
            NativeProtocol::GoogleGemini,
            Some("https://generativelanguage.googleapis.com".into()),
            "GEMINI_MODEL",
            setting("GEMINI_API_KEY"),
        ),
        (
            NativeProtocol::AwsBedrock,
            bedrock,
            "BEDROCK_MODEL",
            bedrock_key(),
        ),
        (
            NativeProtocol::GoogleVertexAi,
            vertex,
            "VERTEX_MODEL",
            setting("GOOGLE_VERTEX_ACCESS_TOKEN"),
        ),
        (
            NativeProtocol::AzureOpenai,
            setting("AZURE_OPENAI_ENDPOINT"),
            "AZURE_OPENAI_DEPLOYMENT",
            setting("AZURE_OPENAI_API_KEY"),
        ),
        (
            NativeProtocol::Ollama,
            setting("OLLAMA_HOST"),
            "OLLAMA_MODEL",
            None,
        ),
        (
            NativeProtocol::Cohere,
            Some("https://api.cohere.com".into()),
            "COHERE_MODEL",
            setting("COHERE_API_KEY"),
        ),
    ];

    let client = UnifiedClient::new();
    let (mut passed, mut skipped, mut failed) = (0, 0, 0);
    for (protocol, endpoint, model_var, key) in probes {
        let name = protocol.as_str();
        let (Some(endpoint), Some(model)) = (endpoint, setting(model_var)) else {
            println!("[SKIP] {name} (endpoint configuration or {model_var} missing)");
            skipped += 1;
            continue;
        };
        if protocol != NativeProtocol::Ollama && key.is_none() {
            println!("[SKIP] {name} (credentials missing)");
            skipped += 1;
            continue;
        }
        let request = LmpRequest {
            model,
            messages: vec![LmpMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Reply with the word pong.".into(),
                    signature: None,
                }],
            }],
            max_tokens: Some(128),
            temperature: None,
            tools: vec![],
            tool_choice: None,
            thinking: None,
            response_format: None,
            timeout_ms: Some(30_000),
            provider_extensions: Default::default(),
        };
        match client.complete(protocol, &endpoint, key, &request).await {
            Ok(response) if !response.text().trim().is_empty() => {
                println!("[PASS] {name} (SDK completion produced text)");
                passed += 1;
            }
            outcome => {
                // Upstream error bodies, endpoints, and responses may contain credentials.
                if let Err(lingxi_core::lmp::LmpError::ProviderError { status, .. }) = outcome {
                    println!("[FAIL] {name} (HTTP {status})");
                } else {
                    println!("[FAIL] {name} (request failed or empty completion)");
                }
                failed += 1;
            }
        }
    }
    println!("Completion probes: {passed} passed, {skipped} skipped, {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
