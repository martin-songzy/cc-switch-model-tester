//! Mock Server 集成测试（DevelopmentPlan.md 19.2）。
//!
//! 本地 TCP Mock 服务器（不调用真实供应商），通过真实 HTTP 客户端
//! 验证「请求构造 → 发送 → 响应解析」端到端行为。

use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use cc_switch_model_tester_lib::domain::{
    ApiProtocol, AppType, CredentialKind, CredentialValue, TestMode, TestTarget,
};
use cc_switch_model_tester_lib::http::{HttpClient, RequestLimits};
use cc_switch_model_tester_lib::protocol::{adapter_for, build_request};

/// 极简 Mock：接受一个连接，记录请求原文，返回固定响应。
fn spawn_mock(status: u16, content_type: &'static str, body: &'static [u8]) -> (String, Arc<Mutex<Vec<u8>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let captured2 = captured.clone();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = vec![0u8; 65536];
            if let Ok(n) = stream.read(&mut buf) {
                buf.truncate(n);
                *captured2.lock().unwrap() = buf;
            }
            let head = format!(
                "HTTP/1.1 {status} TEST\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        }
        // 多余连接直接拒绝（客户端连接池可能尝试复用）
        drop(listener);
    });
    (format!("http://{addr}"), captured)
}

fn target(protocol: ApiProtocol, base: String) -> TestTarget {
    TestTarget {
        app: AppType::Pi,
        provider_id: "p1".into(),
        provider_name: "P1".into(),
        model_id: "model-x".into(),
        model_display_name: None,
        endpoint_url: base,
        protocol,
        credential: Some(CredentialValue {
            kind: match protocol {
                ApiProtocol::GeminiNative => CredentialKind::GeminiKey,
                _ => CredentialKind::BearerKey,
            },
            secret: "sk-test-secret".into(),
        }),
        headers: vec![],
        custom_user_agent: None,
        emulation: false,
            emulation_profile: None,
        client_emulation: None,
        local_proxy_body_patch: None,
        full_url: false,
        compat: serde_json::Value::Null,
    }
}

async fn run(protocol: ApiProtocol, base: String, mode: TestMode) -> cc_switch_model_tester_lib::http::ExecutionOutput {
    let client = HttpClient::new(None, &RequestLimits::default()).unwrap();
    let t = target(protocol, base);
    let prepared = build_request(&t, "你好，请回答一个编程问题。", mode, 256).unwrap();
    let adapter = adapter_for(protocol);
    client
        .execute(&prepared, adapter.as_ref(), mode, &RequestLimits::default())
        .await
}

// ==================== 成功场景 ====================

#[tokio::test]
async fn anthropic_non_stream_success() {
    let body: &'static [u8] = r#"{"content":[{"type":"text","text":"Rust 的 Result 用于可恢复错误，Option 用于可能缺失的值。"}],"stop_reason":"end_turn"}"#.as_bytes();
    let (base, _captured) = spawn_mock(200, "application/json", body);
    let out = run(ApiProtocol::AnthropicMessages, base, TestMode::NonStreaming).await;
    assert_eq!(out.http_status, Some(200));
    if let Some(e) = &out.network_error { panic!("network_error: {e}"); }
    assert!(out.parsed.text.contains("Result"), "text={}", out.parsed.text);
    assert!(out.parsed.finish_signal);
    assert!(out.parsed.error_object.is_none());
    assert!(out.first_byte_ms.is_some());
}

#[tokio::test]
async fn anthropic_stream_success() {
    let body: &'static [u8] = b"event: message_start\ndata: {}\n\nevent: content_block_delta\ndata: {\"delta\":{\"text\":\"Streaming \"}}\n\nevent: content_block_delta\ndata: {\"delta\":{\"text\":\"OK\"}}\n\nevent: message_stop\ndata: {}\n\n";
    let (base, _) = spawn_mock(200, "text/event-stream", body);
    let out = run(ApiProtocol::AnthropicMessages, base, TestMode::Streaming).await;
    assert_eq!(out.http_status, Some(200));
    assert!(out.parsed.text.contains("Streaming OK"), "text={}", out.parsed.text);
    assert!(out.parsed.finish_signal);
}

#[tokio::test]
async fn openai_chat_non_stream_success() {
    let body: &'static [u8] = r#"{"choices":[{"message":{"content":"Chat 回答"},"finish_reason":"stop"}]}"#.as_bytes();
    let (base, _) = spawn_mock(200, "application/json", body);
    let out = run(ApiProtocol::OpenAiChat, base, TestMode::NonStreaming).await;
    assert_eq!(out.parsed.text, "Chat 回答");
    assert_eq!(out.parsed.stop_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn openai_chat_stream_success() {
    let body: &'static [u8] = "data: {\"choices\":[{\"delta\":{\"content\":\"流式\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"回答\"}}]}\n\ndata: [DONE]\n\n".as_bytes();
    let (base, _) = spawn_mock(200, "text/event-stream", body);
    let out = run(ApiProtocol::OpenAiChat, base, TestMode::Streaming).await;
    assert_eq!(out.parsed.text, "流式回答");
    assert!(out.parsed.finish_signal);
}

#[tokio::test]
async fn openai_responses_stream_success() {
    let body: &'static [u8] = b"event: response.output_text.delta\ndata: {\"delta\":\"Resp\"}\n\nevent: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n";    let (base, _) = spawn_mock(200, "text/event-stream", body);
    let out = run(ApiProtocol::OpenAiResponses, base, TestMode::Streaming).await;
    assert_eq!(out.parsed.text, "Resp");
    assert!(out.parsed.finish_signal);
}

#[tokio::test]
async fn gemini_non_stream_and_stream() {
    let body: &'static [u8] = r#"{"candidates":[{"content":{"parts":[{"text":"Gemini 回答"}]},"finishReason":"STOP"}]}"#.as_bytes();
    let (base, _) = spawn_mock(200, "application/json", body);
    let out = run(ApiProtocol::GeminiNative, base, TestMode::NonStreaming).await;
    assert_eq!(out.parsed.text, "Gemini 回答");

    // 流式：连续 JSON 对象
    let body2: &'static [u8] = "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"多\"}]}}]}\n{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"对象\"}]},\"finishReason\":\"STOP\"}]}\n".as_bytes();
    let (base2, _) = spawn_mock(200, "application/json", body2);
    let out2 = run(ApiProtocol::GeminiNative, base2, TestMode::Streaming).await;
    assert_eq!(out2.parsed.text, "多对象");
    assert!(out2.parsed.finish_signal);
}

// ==================== 异常场景 ====================

#[tokio::test]
async fn status200_with_api_error_body_is_flagged() {
    let body: &'static [u8] = br#"{"error":{"message":"api error","code":"internal"}}"#;
    let (base, _) = spawn_mock(200, "application/json", body);
    let out = run(ApiProtocol::OpenAiChat, base, TestMode::NonStreaming).await;
    assert_eq!(out.http_status, Some(200));
    // 200 不代表成功：解析层必须暴露 error 对象，M4 判定引擎据此判失败
    assert!(out.parsed.error_object.unwrap().contains("api error"));
}

#[tokio::test]
async fn status200_empty_content_is_flagged() {
    let body: &'static [u8] = br#"{"choices":[{"message":{"content":""},"finish_reason":"stop"}]}"#;
    let (base, _) = spawn_mock(200, "application/json", body);
    let out = run(ApiProtocol::OpenAiChat, base, TestMode::NonStreaming).await;
    assert!(!out.parsed.has_text(), "200 + 空输出必须可见");
}

#[tokio::test]
async fn http_401_recorded() {
    let body: &'static [u8] = br#"{"error":{"message":"invalid api key"}}"#;
    let (base, _) = spawn_mock(401, "application/json", body);
    let out = run(ApiProtocol::OpenAiChat, base, TestMode::NonStreaming).await;
    assert_eq!(out.http_status, Some(401));
    assert!(out.parsed.error_object.is_some());
}

#[tokio::test]
async fn connection_refused_is_network_error() {
    // 指向一个大概率无服务的端口
    let client = HttpClient::new(None, &RequestLimits {
        connect_timeout: std::time::Duration::from_secs(2),
        total_timeout: std::time::Duration::from_secs(5),
        idle_timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    let t = target(ApiProtocol::OpenAiChat, "http://127.0.0.1:1/v1".into());
    let prepared = build_request(&t, "hi", TestMode::NonStreaming, 256).unwrap();
    let adapter = adapter_for(ApiProtocol::OpenAiChat);
    let out = client
        .execute(&prepared, adapter.as_ref(), TestMode::NonStreaming, &RequestLimits::default())
        .await;
    assert!(out.network_error.is_some(), "连接失败必须产生网络错误");
    assert!(out.http_status.is_none());
}

#[tokio::test]
async fn request_headers_reach_mock() {
    let body: &'static [u8] = br#"{"choices":[{"message":{"content":"ok"}}]}"#;
    let (base, captured) = spawn_mock(200, "application/json", body);
    let out = run(ApiProtocol::OpenAiChat, base, TestMode::NonStreaming).await;
    assert_eq!(out.parsed.text, "ok");
    let raw = String::from_utf8_lossy(&captured.lock().unwrap()).to_lowercase();
    assert!(raw.contains("authorization: bearer sk-test-secret"), "认证 Header 应到达服务器");
    assert!(raw.contains("content-type: application/json"));
    assert!(raw.contains("user-agent: cc-switch-model-tester/"), "应携带应用 UA");
}

#[tokio::test]
async fn custom_user_agent_overrides() {
    let body: &'static [u8] = br#"{"choices":[{"message":{"content":"ok"}}]}"#;
    let (base, captured) = spawn_mock(200, "application/json", body);
    let client = HttpClient::new(None, &RequestLimits::default()).unwrap();
    let mut t = target(ApiProtocol::OpenAiChat, base);
    t.custom_user_agent = Some("claude-cli/9.9 (test)".into());
    let prepared = build_request(&t, "hi", TestMode::NonStreaming, 256).unwrap();
    let adapter = adapter_for(ApiProtocol::OpenAiChat);
    let _ = client.execute(&prepared, adapter.as_ref(), TestMode::NonStreaming, &RequestLimits::default()).await;
    let raw = String::from_utf8_lossy(&captured.lock().unwrap()).to_lowercase();
    assert!(raw.contains("user-agent: claude-cli/9.9"), "customUserAgent 应覆盖默认 UA");
    assert!(!raw.contains("cc-switch-model-tester/"));
}
