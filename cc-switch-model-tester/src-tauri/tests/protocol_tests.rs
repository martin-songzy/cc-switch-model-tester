//! 协议适配器单元测试（DevelopmentPlan.md 19.1 相关项）：
//! 四类协议的请求体构造、URL 规则、非流式/流式解析、200+error、空输出、未知事件容忍。

use cc_switch_model_tester_lib::domain::{
    ApiProtocol, AppType, CredentialKind, CredentialValue, TestMode, TestTarget,
};
use cc_switch_model_tester_lib::protocol::{adapter_for, build_request};

fn target(protocol: ApiProtocol, base: &str) -> TestTarget {
    TestTarget {
        app: AppType::Pi,
        provider_id: "p1".into(),
        provider_name: "P1".into(),
        model_id: "model-x".into(),
        model_display_name: None,
        endpoint_url: base.into(),
        protocol,
        credential: Some(CredentialValue {
            kind: CredentialKind::BearerKey,
            secret: "sk-test".into(),
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

// ==================== URL 规则（7.2）====================

#[test]
fn url_rules_all_protocols() {
    let cases: Vec<(ApiProtocol, &str, TestMode, &str)> = vec![
        (
            ApiProtocol::AnthropicMessages,
            "https://api.a.com",
            TestMode::NonStreaming,
            "https://api.a.com/v1/messages",
        ),
        // base 已带 /v1 不重复追加
        (
            ApiProtocol::AnthropicMessages,
            "https://k40.example.cn",
            TestMode::NonStreaming,
            "https://k40.example.cn/v1/messages",
        ),
        (
            ApiProtocol::AnthropicMessages,
            "https://api.a.com/v1",
            TestMode::Streaming,
            "https://api.a.com/v1/messages",
        ),
        (
            ApiProtocol::OpenAiChat,
            "https://api.a.com/v1",
            TestMode::NonStreaming,
            "https://api.a.com/v1/chat/completions",
        ),
        (
            ApiProtocol::OpenAiResponses,
            "https://api.a.com/v1",
            TestMode::Streaming,
            "https://api.a.com/v1/responses",
        ),
        (
            ApiProtocol::GeminiNative,
            "https://g.example",
            TestMode::NonStreaming,
            "https://g.example/v1beta/models/model-x:generateContent",
        ),
        (
            ApiProtocol::GeminiNative,
            "https://g.example/v1beta",
            TestMode::Streaming,
            "https://g.example/v1beta/models/model-x:streamGenerateContent",
        ),
        // 保留 query
        (
            ApiProtocol::OpenAiChat,
            "https://api.a.com/v1?api-version=x",
            TestMode::NonStreaming,
            "https://api.a.com/v1/chat/completions?api-version=x",
        ),
    ];
    for (protocol, base, mode, expect) in cases {
        let t = target(protocol, base);
        let req = build_request(&t, "hi", mode, 256).unwrap();
        assert_eq!(req.url, expect, "protocol={protocol:?} base={base}");
    }
}

#[test]
fn url_full_url_flag() {
    let mut t = target(ApiProtocol::OpenAiChat, "https://gw.example/custom/path?key=1");
    t.full_url = true;
    let req = build_request(&t, "hi", TestMode::NonStreaming, 256).unwrap();
    assert_eq!(req.url, "https://gw.example/custom/path?key=1");
}

#[test]
fn url_rejects_non_http_scheme() {
    let t = target(ApiProtocol::OpenAiChat, "ftp://x.com");
    assert!(build_request(&t, "hi", TestMode::NonStreaming, 256).is_err());
}

// ==================== 请求体构造（7.5）====================

#[test]
fn anthropic_request_body() {
    let t = target(ApiProtocol::AnthropicMessages, "https://api.a.com");
    let req = build_request(&t, "你好", TestMode::NonStreaming, 256).unwrap();
    assert_eq!(req.body["model"], "model-x");
    assert_eq!(req.body["max_tokens"], 256);
    assert_eq!(req.body["stream"], false);
    assert_eq!(req.body["messages"][0]["role"], "user");
    assert_eq!(req.body["messages"][0]["content"], "你好");
    // anthropic-version header
    assert!(req.headers.iter().any(|(k, v)| k == "anthropic-version" && v == "2023-06-01"));
    // 流式切换
    let req_s = build_request(&t, "hi", TestMode::Streaming, 256).unwrap();
    assert_eq!(req_s.body["stream"], true);
}

#[test]
fn openai_chat_body_and_compat_max_tokens_field() {
    let t = target(ApiProtocol::OpenAiChat, "https://api.a.com/v1");
    let req = build_request(&t, "hi", TestMode::NonStreaming, 256).unwrap();
    assert_eq!(req.body["max_tokens"], 256);
    assert_eq!(req.body["messages"][0]["content"], "hi");
    assert!(req.body.get("store").is_none());

    let mut t2 = t.clone();
    t2.compat = serde_json::json!({ "maxTokensField": "max_completion_tokens" });
    let req2 = build_request(&t2, "hi", TestMode::NonStreaming, 256).unwrap();
    assert!(req2.body.get("max_tokens").is_none());
    assert_eq!(req2.body["max_completion_tokens"], 256);
}

#[test]
fn openai_responses_body_and_store_compat() {
    let t = target(ApiProtocol::OpenAiResponses, "https://api.a.com/v1");
    let req = build_request(&t, "hi", TestMode::NonStreaming, 256).unwrap();
    assert_eq!(req.body["store"], false);
    assert_eq!(req.body["max_output_tokens"], 256);
    assert_eq!(req.body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(req.body["input"][0]["content"][0]["text"], "hi");

    let mut t2 = t.clone();
    t2.compat = serde_json::json!({ "supportsStore": false });
    let req2 = build_request(&t2, "hi", TestMode::NonStreaming, 256).unwrap();
    assert!(req2.body.get("store").is_none(), "compat 禁止 store 时应删除该字段");
}

#[test]
fn gemini_body_and_key_header() {
    let mut t = target(ApiProtocol::GeminiNative, "https://g.example");
    t.credential = Some(CredentialValue {
        kind: CredentialKind::GeminiKey,
        secret: "g-key".into(),
    });
    let req = build_request(&t, "hi", TestMode::NonStreaming, 256).unwrap();
    assert_eq!(req.body["contents"][0]["parts"][0]["text"], "hi");
    assert_eq!(req.body["generationConfig"]["maxOutputTokens"], 256);
    // Key 走 x-goog-api-key Header，不拼入 URL
    assert!(req.headers.iter().any(|(k, v)| k == "x-goog-api-key" && v == "g-key"));
    assert!(!req.url.contains("g-key"));
}

// ==================== 非流式响应解析（8.x）====================

#[test]
fn anthropic_non_stream_success() {
    let a = adapter_for(ApiProtocol::AnthropicMessages);
    let body = serde_json::json!({
        "content": [
            { "type": "text", "text": "第一段" },
            { "type": "text", "text": "第二段" }
        ],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 1, "output_tokens": 2 }
    })
    .to_string();
    let r = a.parse_non_stream(body.as_bytes());
    assert_eq!(r.text, "第一段第二段");
    assert!(r.finish_signal);
    assert_eq!(r.stop_reason.as_deref(), Some("end_turn"));
    assert!(r.error_object.is_none());
}

#[test]
fn openai_chat_non_stream_success() {
    let a = adapter_for(ApiProtocol::OpenAiChat);
    let body = serde_json::json!({
        "choices": [{ "message": { "content": "答案" }, "finish_reason": "stop" }],
        "usage": {}
    })
    .to_string();
    let r = a.parse_non_stream(body.as_bytes());
    assert_eq!(r.text, "答案");
    assert_eq!(r.stop_reason.as_deref(), Some("stop"));
}

#[test]
fn openai_responses_non_stream_success() {
    let a = adapter_for(ApiProtocol::OpenAiResponses);
    let body = serde_json::json!({
        "output": [{
            "content": [{ "type": "output_text", "text": "响应文本" }]
        }],
        "status": "completed"
    })
    .to_string();
    let r = a.parse_non_stream(body.as_bytes());
    assert_eq!(r.text, "响应文本");
    assert_eq!(r.stop_reason.as_deref(), Some("completed"));
}

#[test]
fn gemini_non_stream_success() {
    let a = adapter_for(ApiProtocol::GeminiNative);
    let body = serde_json::json!({
        "candidates": [{
            "content": { "parts": [{ "text": "Gemini 输出" }] },
            "finishReason": "STOP"
        }],
        "usageMetadata": {}
    })
    .to_string();
    let r = a.parse_non_stream(body.as_bytes());
    assert_eq!(r.text, "Gemini 输出");
    assert_eq!(r.stop_reason.as_deref(), Some("STOP"));
}

// ==================== 200 + error 对象 / 空输出（11.4 的解析层基础）====================

#[test]
fn status200_with_error_object_is_detected() {
    let a = adapter_for(ApiProtocol::OpenAiChat);
    let body = serde_json::json!({
        "error": { "message": "api error", "code": "internal" }
    })
    .to_string();
    let r = a.parse_non_stream(body.as_bytes());
    assert!(r.error_object.is_some(), "200 + error 必须被识别");
    assert!(r.error_object.unwrap().contains("api error"));
}

#[test]
fn empty_content_is_detected() {
    let a = adapter_for(ApiProtocol::AnthropicMessages);
    let body = serde_json::json!({ "content": [], "stop_reason": "end_turn" }).to_string();
    let r = a.parse_non_stream(body.as_bytes());
    assert!(!r.has_text(), "空内容必须可见");
    let a2 = adapter_for(ApiProtocol::OpenAiChat);
    let body2 = serde_json::json!({ "choices": [{ "message": { "content": "" }, "finish_reason": "stop" }] })
        .to_string();
    let r2 = a2.parse_non_stream(body2.as_bytes());
    assert!(!r2.has_text());
}

#[test]
fn malformed_json_is_parse_error() {
    let a = adapter_for(ApiProtocol::OpenAiChat);
    let r = a.parse_non_stream(b"not json{");
    assert!(r.parse_error.is_some());
}

// ==================== 流式解析（8.6）====================

#[test]
fn anthropic_stream_parses_and_requires_stop() {
    let a = adapter_for(ApiProtocol::AnthropicMessages);
    let mut p = a.new_stream_parser();
    // 分块喂入（模拟网络分包）
    p.feed_chunk(b"event: content_block_delta\ndata: {\"delta\":{\"text\":\"Hello\"}}\n\n");
    p.feed_chunk(b"event: content_block_delta\ndata: {\"delta\":{\"text\":\" World\"}}\n\nevent: mess");
    p.feed_chunk(b"age_stop\ndata: {}\n\n");
    let r = p.finish();
    assert_eq!(r.text, "Hello World");
    assert!(r.finish_signal, "应收到 message_stop");
    assert!(r.error_object.is_none());
}

#[test]
fn anthropic_stream_without_stop_is_failure() {
    let a = adapter_for(ApiProtocol::AnthropicMessages);
    let mut p = a.new_stream_parser();
    p.feed_chunk("event: content_block_delta\ndata: {\"delta\":{\"text\":\"部分\"}}\n\n".as_bytes());
    let r = p.finish();
    assert!(r.parse_error.is_some(), "缺少完成信号必须判失败");
}

#[test]
fn openai_chat_stream_done_and_error_object() {
    let a = adapter_for(ApiProtocol::OpenAiChat);
    let mut p = a.new_stream_parser();
    p.feed_line("data: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}");
    p.feed_line("");
    p.feed_line("data: {\"choices\":[{\"delta\":{\"content\":\"B\"}}]}");
    p.feed_line("");
    p.feed_line("data: [DONE]");
    p.feed_line("");
    let r = p.finish();
    assert_eq!(r.text, "AB");
    assert!(r.finish_signal);

    // 流中 error 对象优先
    let mut p2 = a.new_stream_parser();
    p2.feed_line("data: {\"error\":{\"message\":\"quota exceeded\"}}");
    p2.feed_line("");
    p2.feed_line("data: [DONE]");
    p2.feed_line("");
    let r2 = p2.finish();
    assert!(r2.error_object.unwrap().contains("quota exceeded"));
}

#[test]
fn openai_responses_stream_completed_and_failed() {
    let a = adapter_for(ApiProtocol::OpenAiResponses);
    let mut p = a.new_stream_parser();
    p.feed_line("event: response.output_text.delta");
    p.feed_line("data: {\"delta\":\"结果\"}");
    p.feed_line("");
    p.feed_line("event: response.completed");
    p.feed_line("data: {\"response\":{\"status\":\"completed\"}}");
    p.feed_line("");
    let r = p.finish();
    assert_eq!(r.text, "结果");
    assert!(r.finish_signal);

    let mut p2 = a.new_stream_parser();
    p2.feed_line("event: response.failed");
    p2.feed_line("data: {\"response\":{\"error\":{\"message\":\"bad\"}}}");
    p2.feed_line("");
    let r2 = p2.finish();
    assert!(r2.error_object.unwrap().contains("bad"));
}

#[test]
fn unknown_events_are_tolerated_and_counted() {
    let a = adapter_for(ApiProtocol::AnthropicMessages);
    let mut p = a.new_stream_parser();
    p.feed_line("event: ping");
    p.feed_line("data: {\"x\":1}");
    p.feed_line("");
    p.feed_line("event: some_future_event");
    p.feed_line("data: {}");
    p.feed_line("");
    p.feed_line("event: content_block_delta");
    p.feed_line("data: {\"delta\":{\"text\":\"ok\"}}");
    p.feed_line("");
    p.feed_line("event: message_stop");
    p.feed_line("data: {}");
    p.feed_line("");
    let r = p.finish();
    assert_eq!(r.text, "ok");
    assert!(r.finish_signal);
    assert_eq!(r.unknown_event_count, 1, "只有未知事件计数，ping/message_start 等已知事件不计");
}

#[test]
fn gemini_stream_multiple_json_objects() {
    let a = adapter_for(ApiProtocol::GeminiNative);
    let mut p = a.new_stream_parser();
    // 连续 JSON 对象（无换行也可）
    p.feed_chunk("{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"甲\"}]}}]}".as_bytes());
    p.feed_chunk("\n{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"乙\"}]},\"finishReason\":\"STOP\"}]}".as_bytes());
    let r = p.finish();
    assert_eq!(r.text, "甲乙");
    assert!(r.finish_signal);
    assert_eq!(r.stop_reason.as_deref(), Some("STOP"));
    assert_eq!(r.event_count, 2);
}

#[test]
fn gemini_stream_sse_form() {
    let a = adapter_for(ApiProtocol::GeminiNative);
    let mut p = a.new_stream_parser();
    p.feed_chunk("data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"SSE\"}]}}]}\n\n".as_bytes());
    p.feed_chunk("data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"流\"}]}}]}\n\n".as_bytes());
    let r = p.finish();
    assert_eq!(r.text, "SSE流");
}

#[test]
fn gemini_prompt_feedback_block_is_error() {
    let a = adapter_for(ApiProtocol::GeminiNative);
    let body = serde_json::json!({
        "promptFeedback": { "blockReason": "SAFETY" }
    })
    .to_string();
    let r = a.parse_non_stream(body.as_bytes());
    assert!(r.error_object.unwrap().contains("SAFETY"));
}

#[test]
fn malformed_sse_stream_is_failure() {
    let a = adapter_for(ApiProtocol::AnthropicMessages);
    let mut p = a.new_stream_parser();
    p.feed_chunk(b"event: content_block_delta\ndata: {broken json!!}\n\n");
    p.feed_chunk(b"event: message_stop\ndata: {}\n\n");
    let r = p.finish();
    // 非法 JSON data 不致命（计未知），但若完成信号收到且无文本 → 解析层不判错，M4 判空失败
    assert!(r.finish_signal);
    assert!(r.unknown_event_count >= 1);
}
