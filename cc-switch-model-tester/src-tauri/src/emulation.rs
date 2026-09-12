//! 出站请求改写层（M6+ 增强）。
//!
//! 在 `protocol::build_request` 产物（PreparedRequest）上、HTTP 执行前应用两类改写：
//!
//! 1. **客户端仿真（clientEmulation）**——复刻 pi 扩展 `clientEmulation-plugins` 的行为：
//!    伪造官方 Agent 客户端指纹（headers + body 特征）以通过中转站的 gate 校验。
//!    三个内置画像与扩展 profiles.json 逐字段对齐：
//!    - `claude-code`（anthropic-messages）
//!    - `codex`（openai-responses）
//!    - `gemini-cli`（google-generative-ai / openai-completions）
//!
//! 2. **cc-switch 本地代理语义（localProxyRequestOverrides）**——供应商 meta 里的
//!    `body` patch 按深度合并语义应用到请求体（保护顶层 `stream` 字段）。
//!
//! 所有改写只作用于本工具发出的测试请求，绝不写回 cc-switch 配置。

use serde_json::{json, Map, Value};

use crate::domain::{ApiProtocol, TestTarget};
use crate::protocol::PreparedRequest;

// ==================== 画像定义 ====================

/// body 补丁类型（与扩展 index.ts 的 BodyPatch.type 对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyPatch {
    AnthropicMetadataUserId,
    PrependIdentitySystemBlock,
    OpenaiInstructionsIdentity,
    CodexSessionHeaders,
    CodexPromptCacheKey,
    CodexIncludeReasoning,
}

/// 一个客户端仿真画像（对应 profiles.json 的一个条目）
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: &'static str,
    /// 仅对这些协议族生效；空 = 通用
    pub match_apis: &'static [ApiProtocol],
    /// 静态请求头（键已小写）
    pub headers: &'static [(&'static str, &'static str)],
    /// 身份短句候选；第一项用于注入
    pub identity_lines: &'static [&'static str],
    /// body 补丁，按顺序应用
    pub body_patches: &'static [BodyPatch],
}

const ANTHROPIC_MESSAGES: &[ApiProtocol] = &[ApiProtocol::AnthropicMessages];
const OPENAI_RESPONSES: &[ApiProtocol] = &[ApiProtocol::OpenAiResponses];
const GEMINI_OPENAI: &[ApiProtocol] = &[ApiProtocol::GeminiNative, ApiProtocol::OpenAiChat];

/// 内置画像表（与 clientEmulation-plugins/extension/profiles.json 对齐）
pub static PROFILES: &[Profile] = &[
    Profile {
        name: "claude-code",
        match_apis: ANTHROPIC_MESSAGES,
        headers: &[
            ("user-agent", "claude-cli/2.1.252 (external, sdk-cli)"),
            ("x-app", "cli"),
            ("anthropic-beta", "claude-code-20250219"),
            ("anthropic-version", "2023-06-01"),
            ("anthropic-dangerous-direct-browser-access", "true"),
        ],
        identity_lines: &[
            "You are a Claude agent, built on Anthropic's Claude Agent SDK.",
            "You are Claude Code, Anthropic's official CLI for Claude.",
        ],
        body_patches: &[BodyPatch::AnthropicMetadataUserId, BodyPatch::PrependIdentitySystemBlock],
    },
    Profile {
        name: "codex",
        match_apis: OPENAI_RESPONSES,
        headers: &[
            (
                "user-agent",
                "codex_exec/0.153.4 (Windows 10.0.19045; x86_64) WindowsTerminal (codex_exec; 0.153.4)",
            ),
            ("originator", "codex_exec"),
            ("accept", "text/event-stream"),
            ("x-codex-beta-features", "remote_compaction_v2"),
            ("x-openai-internal-codex-responses-lite", "true"),
        ],
        identity_lines: &[
            "You are Codex, a coding agent.",
            "You are Codex, a coding agent. You and the user share the same workspace.",
        ],
        body_patches: &[
            BodyPatch::CodexSessionHeaders,
            BodyPatch::CodexPromptCacheKey,
            BodyPatch::CodexIncludeReasoning,
        ],
    },
    Profile {
        name: "gemini-cli",
        match_apis: GEMINI_OPENAI,
        headers: &[
            ("user-agent", "GeminiCLI/0.1.0 (win32; x64)"),
            ("x-goog-api-client", "gl-node/24.14.1"),
        ],
        identity_lines: &[
            "You are an interactive CLI agent specializing in software engineering tasks.",
        ],
        body_patches: &[],
    },
];

/// 手动开启仿真、且供应商未带配置时，按协议族选默认画像
pub fn profile_for_protocol(protocol: ApiProtocol) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.match_apis.contains(&protocol))
}

/// 依名字查画像
fn profile_by_name(name: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.name == name)
}

/// 本目标应使用的画像：
/// 1. 用户显式选择的画像（emulation_profile）→ 无条件使用（跨协议由用户自行判断）
/// 2. 供应商 clientEmulation 配置的 profile
/// 3. 按协议族默认画像
pub fn resolve_profile(target: &TestTarget) -> Option<&'static Profile> {
    if !target.emulation {
        return None;
    }
    let name = target
        .emulation_profile
        .clone()
        .or_else(|| {
            target
                .client_emulation
                .as_ref()
                .map(|s| s.profile.clone())
        })
        .unwrap_or_else(|| {
            profile_for_protocol(target.protocol)
                .map(|p| p.name)
                .unwrap_or("")
                .to_string()
        });
    profile_by_name(&name)
}

// ==================== 设备指纹 ====================

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 本机主机名（Windows 用 COMPUTERNAME，兜底 "localhost"）
fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string())
}

/// 与扩展一致：sha256("pi-client-emulation:" + hostname)
fn device_id() -> String {
    sha256_hex(format!("pi-client-emulation:{}", hostname()).as_bytes())
}

/// 稳定伪 installation_id（UUID 形式，绑定主机名）
fn installation_id() -> String {
    let h = sha256_hex(format!("pi-codex-install:{}", hostname()).as_bytes());
    format!(
        "{}-{}-4{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        format!("{:x}", (u8::from_str_radix(&h[16..17], 16).unwrap_or(0) & 0x3) | 0x8) + &h[17..20],
        &h[20..32],
    )
}

/// 伪 UUID v4（由输入派生；格式合规即可，不追求密码学随机）
fn pseudo_uuid(seed: &str) -> String {
    let h = sha256_hex(seed.as_bytes());
    format!(
        "{}-{}-4{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        format!("{:x}", (u8::from_str_radix(&h[16..17], 16).unwrap_or(0) & 0x3) | 0x8) + &h[17..20],
        &h[20..32],
    )
}

// ==================== body 补丁 ====================

fn is_plain_object(v: &Value) -> bool {
    v.is_object()
}

/// metadata.user_id：含 device_id + account_uuid + session_id 的内嵌 JSON 字符串（幂等）
fn patch_anthropic_metadata_user_id(body: &mut Map<String, Value>, session_id: &str) -> bool {
    let meta = match body.get_mut("metadata") {
        Some(m) if is_plain_object(m) => m.as_object_mut().unwrap(),
        _ => {
            body.insert("metadata".into(), json!({}));
            body.get_mut("metadata").unwrap().as_object_mut().unwrap()
        }
    };
    let valid = meta.get("user_id").and_then(|v| v.as_str()).is_some_and(|s| {
        serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|p| p.as_object().map(|o| o.to_owned()))
            .is_some_and(|p| {
                p.get("device_id").and_then(|d| d.as_str()).is_some_and(|d| !d.is_empty())
                    && p.contains_key("account_uuid")
                    && p.get("session_id").and_then(|s| s.as_str()).is_some_and(|s| !s.is_empty())
            })
    });
    if valid {
        return false;
    }
    meta.insert(
        "user_id".into(),
        json!(serde_json::to_string(&json!({
            "device_id": device_id(),
            "account_uuid": "",
            "session_id": session_id,
        })).unwrap_or_default()),
    );
    true
}

/// system 首块必须恰为身份短句、独立成块（幂等）
fn patch_prepend_identity_system_block(
    body: &mut Map<String, Value>,
    identity_lines: &[&str],
) -> bool {
    let Some(primary) = identity_lines.first() else { return false };
    let known: Vec<&str> = identity_lines.iter().map(|l| l.trim()).collect();
    let identity_block = json!({ "type": "text", "text": primary });

    let first_is_identity = |v: Option<&Value>| -> bool {
        v.and_then(|v| v.as_object())
            .and_then(|o| o.get("text"))
            .and_then(|t| t.as_str())
            .is_some_and(|t| known.contains(&t.trim()))
    };

    match body.get("system") {
        Some(Value::String(s)) => {
            let s = s.clone();
            body.insert(
                "system".into(),
                if s.trim().is_empty() {
                    json!([identity_block])
                } else {
                    json!([identity_block, { "type": "text", "text": s }])
                },
            );
            true
        }
        Some(Value::Array(arr)) if arr.is_empty() || !first_is_identity(arr.first()) => {
            let mut new_arr = vec![identity_block];
            new_arr.extend(arr.iter().cloned());
            body.insert("system".into(), Value::Array(new_arr));
            true
        }
        _ => false,
    }
}

/// OpenAI 系：确保 instructions 以身份句开头（幂等）
#[allow(dead_code)]
fn patch_openai_instructions_identity(
    body: &mut Map<String, Value>,
    identity_lines: &[&str],
) -> bool {
    let Some(primary) = identity_lines.first() else { return false };
    let mut prefixes: Vec<String> = Vec::new();
    for line in identity_lines {
        let t = line.trim();
        if t.is_empty() { continue; }
        prefixes.push(t.to_string());
        if let Some(head) = t.split(['.', ',', '，', '。']).next() {
            let head = head.trim();
            if !head.is_empty() { prefixes.push(head.to_string()); }
        }
    }
    match body.get("instructions") {
        Some(Value::String(cur)) => {
            let trimmed = cur.trim_start();
            if prefixes.iter().any(|p| trimmed.starts_with(p.as_str())) {
                return false;
            }
            body.insert("instructions".into(), json!(format!("{primary}\n\n{cur}")));
            true
        }
        None => {
            body.insert("instructions".into(), json!(primary));
            true
        }
        _ => false,
    }
}

// ==================== 画像应用 ====================

/// 对 PreparedRequest 应用客户端仿真画像。返回实际生效的补丁名（诊断用）。
pub fn apply_client_emulation(
    req: &mut PreparedRequest,
    target: &TestTarget,
    profile: &Profile,
    session_id: &str,
) -> Vec<String> {
    let mut applied: Vec<String> = Vec::new();
    let mut patch_headers: Vec<(String, String)> = Vec::new();

    // ---- body 补丁（仅 JSON 对象体）
    if let Value::Object(map) = &mut req.body {
        for patch in profile.body_patches {
            let changed = match patch {
                BodyPatch::AnthropicMetadataUserId => {
                    patch_anthropic_metadata_user_id(map, session_id)
                }
                BodyPatch::PrependIdentitySystemBlock => {
                    patch_prepend_identity_system_block(map, profile.identity_lines)
                }
                BodyPatch::OpenaiInstructionsIdentity => {
                    patch_openai_instructions_identity(map, profile.identity_lines)
                }
                BodyPatch::CodexSessionHeaders => {
                    let turn_id = pseudo_uuid(&format!("{session_id}:turn"));
                    let turn_started = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    patch_headers.push(("session-id".into(), session_id.into()));
                    patch_headers.push(("thread-id".into(), session_id.into()));
                    patch_headers.push(("x-client-request-id".into(), session_id.into()));
                    patch_headers.push(("x-codex-window-id".into(), format!("{session_id}:0")));
                    patch_headers.push((
                        "x-codex-turn-metadata".into(),
                        serde_json::to_string(&json!({
                            "installation_id": installation_id(),
                            "session_id": session_id,
                            "thread_id": session_id,
                            "agent_name": "/root",
                            "turn_id": turn_id,
                            "window_id": format!("{session_id}:0"),
                            "window_number": 0,
                            "request_kind": "turn",
                            "root_turn_id": turn_id,
                            "thread_source": "user",
                            "sandbox": "none",
                            "turn_started_at_unix_ms": turn_started,
                        }))
                        .unwrap_or_default(),
                    ));
                    true
                }
                BodyPatch::CodexPromptCacheKey => {
                    let missing_or_nonstr = !map.get("prompt_cache_key").is_some_and(|v| v.is_string());
                    if missing_or_nonstr {
                        map.insert("prompt_cache_key".into(), json!(session_id));
                        true
                    } else {
                        false
                    }
                }
                BodyPatch::CodexIncludeReasoning => {
                    const MARK: &str = "reasoning.encrypted_content";
                    match map.get_mut("include") {
                        Some(Value::Array(arr)) => {
                            let has = arr.iter().any(|v| v.as_str() == Some(MARK));
                            if !has {
                                arr.push(json!(MARK));
                                true
                            } else {
                                false
                            }
                        }
                        _ => {
                            map.insert("include".into(), json!([MARK]));
                            true
                        }
                    }
                }
            };
            if changed {
                applied.push(format!("{patch:?}"));
            }
        }
    }

    // ---- headers 合并：profile 静态头 → patch 头 → 供应商覆盖（userAgent / spec.headers）
    //     规则（与扩展一致）：null = 删除；patch 产生的头强制覆盖；
    //     UA 已含官方指纹时保留（除非供应商显式覆盖）。
    let explicit_ua = target
        .client_emulation
        .as_ref()
        .and_then(|s| s.user_agent.clone());

    // profile 静态头 + 供应商覆盖头
    let mut merged: Vec<(String, Option<String>)> = Vec::new();
    for (k, v) in profile.headers {
        merged.push((k.to_string(), Some(v.to_string())));
    }
    if let Some(spec) = &target.client_emulation {
        for (k, v) in &spec.headers {
            merged.push((k.to_lowercase(), v.clone()));
        }
    }
    if let Some(ua) = explicit_ua {
        merged.push(("user-agent".into(), Some(ua)));
    }

    set_emulation_headers(req, &merged, &patch_headers);
    applied
}

/// headers 写入（合并规则）
fn set_emulation_headers(
    req: &mut PreparedRequest,
    merged: &[(String, Option<String>)],
    patch_headers: &[(String, String)],
) {
    for (k, v) in merged {
        match v {
            None => req.headers.retain(|(hk, _)| hk.to_lowercase() != *k),
            Some(vv) => {
                if k == "user-agent" {
                    // 已有合规 UA 则保留（除非供应商显式覆盖——上面 merged 已带 override）
                    let cur = req
                        .headers
                        .iter()
                        .find(|(hk, _)| hk.eq_ignore_ascii_case("user-agent"))
                        .map(|(_, hv)| hv.clone())
                        .unwrap_or_default();
                    let looks_official = |s: &str| {
                        s.contains("claude-cli/")
                            || s.contains("codex_cli_rs/")
                            || s.contains("GeminiCLI/")
                            || s.contains("codex_exec/")
                    };
                    if looks_official(&cur) {
                        continue;
                    }
                }
                if let Some(h) = req.headers.iter_mut().find(|(hk, _)| hk.eq_ignore_ascii_case(k)) {
                    h.1 = vv.clone();
                } else {
                    req.headers.push((k.clone(), vv.clone()));
                }
            }
        }
    }
    for (k, v) in patch_headers {
        if let Some(h) = req.headers.iter_mut().find(|(hk, _)| hk.eq_ignore_ascii_case(&k)) {
            h.1 = v.clone();
        } else {
            req.headers.push((k.clone(), v.clone()));
        }
    }
}

// ==================== cc-switch overrides.body 深度合并 ====================

/// cc-switch 本地代理 body 改写：深度合并（保护顶层 `stream` 字段）。
/// 返回是否有变更。
pub fn apply_local_proxy_body_patch(body: &mut Value, patch: &Value) -> bool {
    if !patch.is_object() {
        return false;
    }
    merge_json_override(body, patch, true)
}

fn merge_json_override(target: &mut Value, patch: &Value, is_top_level: bool) -> bool {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            let mut changed = false;
            for (key, patch_value) in patch_map {
                if is_top_level && key == "stream" {
                    continue; // 受保护字段
                }
                match target_map.get_mut(key) {
                    Some(target_value) => {
                        changed |= merge_json_override(target_value, patch_value, false);
                    }
                    None => {
                        target_map.insert(key.clone(), patch_value.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        (target_value, patch_value) => {
            if target_value == patch_value {
                false
            } else {
                *target_value = patch_value.clone();
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ApiProtocol, TestTarget};
    use serde_json::json;

    fn claude_target(emulation: bool, spec: Option<crate::domain::ClientEmulationSpec>) -> TestTarget {
        TestTarget {
            app: crate::domain::AppType::Pi,
            provider_id: "p".into(),
            provider_name: "P".into(),
            model_id: "m".into(),
            model_display_name: None,
            endpoint_url: "https://relay-a.example.com".into(),
            protocol: ApiProtocol::AnthropicMessages,
            credential: None,
            headers: vec![],
            custom_user_agent: None,
            emulation,
            emulation_profile: None,
            client_emulation: spec,
            local_proxy_body_patch: None,
            full_url: false,
            compat: serde_json::Value::Null,
        }
    }

    fn prepared(target: &TestTarget) -> PreparedRequest {
        PreparedRequest {
            url: "https://relay-a.example.com/v1/messages".into(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: json!({
                "model": "m",
                "max_tokens": 64,
                "stream": false,
                "system": [{"type": "text", "text": "回答 pong"}],
                "messages": [{"role": "user", "content": "hi"}]
            }),
        }
    }

    #[test]
    fn claude_code_profile_applies_headers_and_body() {
        let t = claude_target(true, None);
        let profile = resolve_profile(&t).unwrap();
        assert_eq!(profile.name, "claude-code");
        let mut req = prepared(&t);
        let applied = apply_client_emulation(&mut req, &t, profile, "run-1");
        assert_eq!(applied.len(), 2);
        // headers
        let ua = req.headers.iter().find(|(k, _)| k == "user-agent").unwrap();
        assert!(ua.1.starts_with("claude-cli/"));
        assert!(req.headers.iter().any(|(k, _)| k == "anthropic-beta"));
        // system 首块 = 身份短句，独立成块
        let sys = req.body["system"].as_array().unwrap();
        assert_eq!(sys.len(), 2);
        assert_eq!(sys[0]["text"], profile.identity_lines[0]);
        assert_eq!(sys[1]["text"], "回答 pong");
        // metadata.user_id 为内嵌 JSON
        let uid = req.body["metadata"]["user_id"].as_str().unwrap();
        let p: Value = serde_json::from_str(uid).unwrap();
        assert!(!p["device_id"].as_str().unwrap().is_empty());
        assert_eq!(p["session_id"], "run-1");
    }

    #[test]
    fn emulation_is_idempotent() {
        let t = claude_target(true, None);
        let profile = resolve_profile(&t).unwrap();
        let mut req = prepared(&t);
        apply_client_emulation(&mut req, &t, profile, "run-1");
        let once = req.body.clone();
        apply_client_emulation(&mut req, &t, profile, "run-1");
        assert_eq!(req.body, once, "二次应用不应改变 body（幂等）");
    }

    #[test]
    fn protocol_default_profile_when_no_spec() {
        // claude 供应商（无 spec）手动开启 → 按协议默认画像
        let t = claude_target(true, None);
        assert_eq!(resolve_profile(&t).unwrap().name, "claude-code");
        // 关闭 → None
        assert!(resolve_profile(&claude_target(false, None)).is_none());
        // openai-responses + 手动开启 → codex 画像
        let mut t2 = claude_target(true, None);
        t2.protocol = ApiProtocol::OpenAiResponses;
        assert_eq!(resolve_profile(&t2).unwrap().name, "codex");
    }

    #[test]
    fn codex_profile_applies_session_fields() {
        let mut t = claude_target(true, None);
        t.protocol = ApiProtocol::OpenAiResponses;
        let profile = resolve_profile(&t).unwrap();
        assert_eq!(profile.name, "codex");
        let mut req = prepared(&t);
        req.body = json!({"model": "m", "stream": false});
        apply_client_emulation(&mut req, &t, profile, "run-9");
        assert_eq!(req.body["prompt_cache_key"], "run-9");
        assert_eq!(req.body["include"][0], "reasoning.encrypted_content");
        assert!(req.headers.iter().any(|(k, _)| k == "session-id"));
        let meta = req.headers.iter().find(|(k, _)| k == "x-codex-turn-metadata").unwrap();
        let m: Value = serde_json::from_str(&meta.1).unwrap();
        assert_eq!(m["session_id"], "run-9");
    }

    #[test]
    fn deep_merge_respects_stream_protection() {
        let mut body = json!({"stream": true, "max_tokens": 16, "thinking": {"type": "disabled"}});
        let patch = json!({"stream": false, "max_tokens": 8192, "thinking": {"type": "enabled", "budget_tokens": 4096}, "temperature": 1});
        assert!(apply_local_proxy_body_patch(&mut body, &patch));
        assert_eq!(body["stream"], true, "顶层 stream 受保护");
        assert_eq!(body["max_tokens"], 8192);
        assert_eq!(body["temperature"], 1);
        assert_eq!(body["thinking"]["budget_tokens"], 4096);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn dedup_key_distinguishes_emulation() {
        let mut t = claude_target(true, None);
        let k_on = crate::dedup::dedup_key_hash(&t, crate::domain::TestMode::NonStreaming);
        t.emulation = false;
        let k_off = crate::dedup::dedup_key_hash(&t, crate::domain::TestMode::NonStreaming);
        assert_ne!(k_on, k_off, "仿真开/关应为不同去重目标");
    }
}
