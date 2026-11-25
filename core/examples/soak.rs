//! Soak driver: sustained loopback load through the Rust core.
//!
//! `cargo run --example soak` expects `LINGXI_SOAK_URL` (the loopback upstream
//! printed by `conformance/soak/soak_server.py`) and `LINGXI_SOAK_SECONDS`
//! (default 1800, i.e. 30 minutes). It cycles every protocol through
//! completion and incremental streaming, samples its own resident size and
//! open file descriptors, and exits non-zero on the first unexpected failure.
//!
//! Credentials are synthetic: the upstream is loopback-only.

use std::{
    collections::HashMap,
    env,
    process::ExitCode,
    time::{Duration, Instant},
};

use lingxi_core::{
    lmp::{
        ContentBlock, EmbeddingRequest, LmpMessage, LmpRequest, LmpStreamEvent, RerankRequest,
        Role, UnifiedClient,
    },
    NativeProtocol,
};

struct Sample {
    at: Duration,
    rss_kib: u64,
    fds: usize,
}

fn setting(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

// Linux and macOS both expose the resident set size in bytes at these paths.
fn resident_kib() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok();
    if let Some(contents) = statm {
        let pages: u64 = contents.split_whitespace().nth(1)?.parse().ok()?;
        return Some(pages * 4);
    }
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn open_fds() -> Option<usize> {
    let entries = std::fs::read_dir("/dev/fd").ok()?;
    Some(entries.count())
}

fn request(model: &str) -> LmpRequest {
    LmpRequest {
        model: model.to_owned(),
        messages: vec![LmpMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Reply with the word pong.".into(),
                signature: None,
            }],
        }],
        max_tokens: Some(64),
        temperature: None,
        tools: vec![],
        tool_choice: None,
        thinking: None,
        response_format: None,
        timeout_ms: Some(15_000),
        provider_extensions: HashMap::new().into_iter().collect(),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let Some(url) = setting("LINGXI_SOAK_URL") else {
        eprintln!("LINGXI_SOAK_URL is required (run conformance/soak/soak_server.py first)");
        return ExitCode::FAILURE;
    };
    let seconds: u64 = setting("LINGXI_SOAK_SECONDS")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1800);
    let protocols = [
        NativeProtocol::OpenaiChatCompletions,
        NativeProtocol::OpenaiResponses,
        NativeProtocol::AnthropicMessages,
        NativeProtocol::GoogleGemini,
        NativeProtocol::AwsBedrock,
        NativeProtocol::GoogleVertexAi,
        NativeProtocol::AzureOpenai,
        NativeProtocol::Ollama,
        NativeProtocol::Cohere,
    ];
    let client = UnifiedClient::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let started = Instant::now();
    let mut samples: Vec<Sample> = Vec::new();
    let (mut completions, mut streams, mut embeddings, mut reranks, mut cancels) =
        (0_u64, 0_u64, 0_u64, 0_u64, 0_u64);
    let mut failures = 0_u64;
    let mut round = 0_u64;
    while Instant::now() < deadline {
        round += 1;
        for protocol in protocols {
            let key = Some("soak-key".to_owned());
            match client
                .complete(protocol, &url, key.clone(), &request("soak"))
                .await
            {
                Ok(response) if !response.text().is_empty() => completions += 1,
                Ok(_) => {
                    failures += 1;
                    eprintln!(
                        "[round {round}] {} completion returned no text",
                        protocol.as_str()
                    );
                }
                Err(error) => {
                    failures += 1;
                    eprintln!(
                        "[round {round}] {} completion failed: {error}",
                        protocol.as_str()
                    );
                }
            }
            match client
                .open_stream(protocol, &url, key.clone(), &request("soak"))
                .await
            {
                Ok(mut handle) => {
                    let mut text = String::new();
                    loop {
                        match handle.next_event().await {
                            Ok(Some(LmpStreamEvent::TextDelta { delta, .. })) => {
                                text.push_str(&delta)
                            }
                            Ok(Some(LmpStreamEvent::Error { code, message })) => {
                                failures += 1;
                                eprintln!(
                                    "[round {round}] {} stream error {code}: {message}",
                                    protocol.as_str()
                                );
                                break;
                            }
                            Ok(Some(_)) => {}
                            Ok(None) => break,
                            Err(error) => {
                                failures += 1;
                                eprintln!(
                                    "[round {round}] {} stream read failed: {error}",
                                    protocol.as_str()
                                );
                                break;
                            }
                        }
                    }
                    if text.trim().is_empty() {
                        failures += 1;
                        eprintln!(
                            "[round {round}] {} stream produced no text",
                            protocol.as_str()
                        );
                    } else {
                        streams += 1;
                    }
                }
                Err(error) => {
                    failures += 1;
                    eprintln!(
                        "[round {round}] {} stream open failed: {error}",
                        protocol.as_str()
                    );
                }
            }
            // Mid-stream cancellation: the upstream stalls on model cancel-1.
            if let Ok(mut handle) = client
                .open_stream(protocol, &url, key.clone(), &request("cancel-1"))
                .await
            {
                // Cancel by token: the reader owns the handle mutably, so the
                // abort path must go through the shared cancel flag + notify
                // pair instead of borrowing the handle again.
                let token = handle.cancellation();
                let notify = handle.cancellation_notify();
                let task = tokio::spawn(async move {
                    let _ = handle.next_event().await;
                });
                tokio::time::sleep(Duration::from_millis(50)).await;
                token.store(true, std::sync::atomic::Ordering::SeqCst);
                notify.notify_waiters();
                let aborted = tokio::time::timeout(Duration::from_secs(5), task)
                    .await
                    .is_ok();
                if aborted {
                    cancels += 1;
                } else {
                    failures += 1;
                    eprintln!(
                        "[round {round}] {} cancel did not interrupt the read",
                        protocol.as_str()
                    );
                }
            }
        }
        let embedding = EmbeddingRequest {
            model: "soak-embed".into(),
            input: vec!["a".into(), "b".into()],
        };
        match client
            .embed(
                NativeProtocol::OpenaiChatCompletions,
                &url,
                Some("soak-key".into()),
                &embedding,
            )
            .await
        {
            Ok(response) if response.embeddings.len() == 2 => embeddings += 1,
            Ok(_) => failures += 1,
            Err(error) => {
                failures += 1;
                eprintln!("[round {round}] embedding failed: {error}");
            }
        }
        let rerank = RerankRequest {
            model: "soak-rerank".into(),
            query: "pong".into(),
            documents: vec!["pong".into(), "other".into()],
            top_n: Some(1),
        };
        match client
            .rerank(
                NativeProtocol::Cohere,
                &url,
                Some("soak-key".into()),
                &rerank,
            )
            .await
        {
            Ok(response) if response.results.len() == 1 => reranks += 1,
            Ok(_) => failures += 1,
            Err(error) => {
                failures += 1;
                eprintln!("[round {round}] rerank failed: {error}");
            }
        }
        if round % 10 == 0 {
            samples.push(Sample {
                at: started.elapsed(),
                rss_kib: resident_kib().unwrap_or(0),
                fds: open_fds().unwrap_or(0),
            });
            let sample = samples.last().expect("just pushed");
            println!(
                "round {round} at {:>6.1}s: rss {} KiB, fds {}, completions {}, streams {}, cancels {}",
                sample.at.as_secs_f64(),
                sample.rss_kib,
                sample.fds,
                completions,
                streams,
                cancels,
            );
        }
        if failures > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    println!(
        "soak finished after {:.1}s: {completions} completions, {streams} streams, {cancels} cancels, {embeddings} embeddings, {reranks} reranks, {failures} failures",
        started.elapsed().as_secs_f64(),
    );
    if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
        let growth = last.rss_kib as i64 - first.rss_kib as i64;
        println!(
            "rss: {} KiB -> {} KiB (delta {growth} KiB), fds: {} -> {}",
            first.rss_kib, last.rss_kib, first.fds, last.fds
        );
        if growth > 64 * 1024 {
            failures += 1;
            eprintln!("resident size grew by more than 64 MiB; suspected leak");
        }
        if last.fds > first.fds + 64 {
            failures += 1;
            eprintln!("open descriptors grew by more than 64; suspected leak");
        }
    }
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
