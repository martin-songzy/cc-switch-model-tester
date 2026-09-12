//! 去重语义测试（DevelopmentPlan.md 19.1 关键项）：
//! 只按精确请求指纹合并；名称/ID 相同不合并；数量统计正确且确定。

use cc_switch_model_tester_lib::dedup::{deduplicate, dedup_key_hash, expand_provider, ExpandedTarget};
use cc_switch_model_tester_lib::domain::{
    ApiProtocol, AppType, ClientEmulationSpec, CredentialKind, CredentialValue, ProviderSnapshot,
    ProviderStatus, TestMode, TestTarget,
};
use std::collections::{HashMap, HashSet};

mod common;
use common::model_snap;

fn snapshot(provider_id: &str, name: &str) -> ProviderSnapshot {
    ProviderSnapshot {
        app: AppType::Pi,
        provider_id: provider_id.into(),
        provider_name: name.into(),
        endpoint: "https://x.example/v1".into(),
        candidate_endpoints: vec![],
        protocol: ApiProtocol::OpenAiChat,
        raw_protocol: None,
        models: vec![model_snap("m1"), model_snap("m2")],
        credential: Some(CredentialValue {
            kind: CredentialKind::BearerKey,
            secret: "sk-A".into(),
        }),
        headers: vec![],
        custom_user_agent: None,
        client_emulation: None,
        local_proxy_body_patch: None,
        full_url: false,
        compat: serde_json::Value::Null,
        passthrough: serde_json::Value::Null,
        raw_config_hash: "h".into(),
        status: ProviderStatus::Ready,
        warnings: vec![],
        config_error: None,
    }
}

fn expand(snap: &ProviderSnapshot, all: bool) -> Vec<ExpandedTarget> {
    expand_provider(snap, &HashSet::new(), 3, TestMode::NonStreaming, all, &std::collections::HashMap::new())
}

#[test]
fn identical_targets_merge() {
    // 两个不同供应商，但端点/协议/模型/模式/认证完全相同 → 去重为 1 组
    let mut s2 = snapshot("p2", "Provider B");
    s2.credential = Some(CredentialValue {
        kind: CredentialKind::BearerKey,
        secret: "sk-A".into(),
    });
    let mut targets = expand(&snapshot("p1", "Provider A"), false);
    targets.extend(expand(&s2, false));
    let (groups, summary) = deduplicate(targets, TestMode::NonStreaming, 3);
    assert_eq!(summary.original_target_count, 4);
    assert_eq!(summary.deduplicated_target_count, 2, "相同指纹的 m1/m2 各自合并");
    assert_eq!(summary.duplicate_group_count, 2);
    assert_eq!(summary.removed_attempt_count, 6, "4 目标 ×3 = 12 → 2 ×3 = 6，减少 6");
    // 被合并来源保留
    assert!(groups.iter().all(|g| g.merged_sources.len() == 2));
}

#[test]
fn different_api_key_does_not_merge() {
    let s1 = snapshot("p1", "A");
    let mut s2 = snapshot("p2", "B");
    s2.credential = Some(CredentialValue {
        kind: CredentialKind::BearerKey,
        secret: "sk-DIFFERENT".into(),
    });
    let mut targets = expand(&s1, false);
    targets.extend(expand(&s2, false));
    let (_, summary) = deduplicate(targets, TestMode::NonStreaming, 3);
    assert_eq!(summary.deduplicated_target_count, 4, "不同 API Key 不合并");
}

#[test]
fn same_name_different_endpoint_does_not_merge() {
    let s1 = snapshot("p1", "Same Name");
    let mut s2 = snapshot("p2", "Same Name");
    s2.endpoint = "https://other.example/v1".into();
    let mut targets = expand(&s1, false);
    targets.extend(expand(&s2, false));
    let (_, summary) = deduplicate(targets, TestMode::NonStreaming, 3);
    assert_eq!(summary.deduplicated_target_count, 4, "同名供应商不同端点不合并");
}

#[test]
fn different_mode_does_not_merge() {
    let s1 = snapshot("p1", "A");
    let mut targets = expand(&s1, false);
    // 相同 target 但模式不同
    for t in expand(&s1, false) {
        targets.push(ExpandedTarget {
            target: t.target,
            source: t.source,
        });
    }
    let (g1, _) = deduplicate(targets.clone(), TestMode::NonStreaming, 3);
    let (g2, s2) = deduplicate(targets, TestMode::Streaming, 3);
    assert_eq!(g1.len(), 2);
    assert_eq!(g2.len(), 2);
    assert_eq!(s2.original_target_count, 4);
    // 同一批内（模式一致）合并，两种模式之间的键不同
    let t = &g1[0].representative;
    assert_ne!(
        dedup_key_hash(t, TestMode::NonStreaming),
        dedup_key_hash(t, TestMode::Streaming)
    );
}

#[test]
fn different_headers_do_not_merge() {
    let mut s1 = snapshot("p1", "A");
    s1.headers = vec![("x-flag".into(), "1".into())];
    let s2 = snapshot("p2", "B");
    let mut targets = expand(&s1, false);
    targets.extend(expand(&s2, false));
    let (_, summary) = deduplicate(targets, TestMode::NonStreaming, 3);
    assert_eq!(summary.deduplicated_target_count, 4, "有效 Header 不同不合并");
}

#[test]
fn candidate_endpoints_expand_and_differ() {
    let mut s1 = snapshot("p1", "A");
    s1.candidate_endpoints = vec!["https://alt.example/v1".into()];
    let targets = expand(&s1, true);
    assert_eq!(targets.len(), 4, "2 模型 × (主端点 + 1 候选)");
    let (_, summary) = deduplicate(targets, TestMode::NonStreaming, 3);
    assert_eq!(summary.deduplicated_target_count, 4);
}

#[test]
fn dedup_is_deterministic() {
    let s1 = snapshot("p1", "A");
    let targets = expand(&s1, false);
    let (g1, _) = deduplicate(targets.clone(), TestMode::NonStreaming, 3);
    let (g2, _) = deduplicate(targets, TestMode::NonStreaming, 3);
    // 代表项选择稳定
    assert_eq!(g1[0].representative.provider_id, g2[0].representative.provider_id);
    assert_eq!(g1[0].representative.model_id, g2[0].representative.model_id);
    assert_eq!(g1[0].key_hash, g2[0].key_hash);
}

#[test]
fn selected_models_filter() {
    let s1 = snapshot("p1", "A");
    let selected: HashSet<String> = ["m1".to_string()].into();
    let targets = expand_provider(&s1, &selected, 3, TestMode::NonStreaming, false, &std::collections::HashMap::new());
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].target.model_id, "m1");
}

#[test]
fn expand_applies_per_model_emulation_overrides() {
    let s1 = snapshot("p1", "A");
    let selected: HashSet<String> = ["m1".to_string()].into();
    // 前端 selectionKey 格式：app::providerId::modelId；显式选画像 → 生效
    let ov: HashMap<String, String> = [("pi::p1::m1".to_string(), "codex".to_string())].into();
    let targets = expand_provider(&s1, &selected, 3, TestMode::NonStreaming, false, &ov);
    assert!(targets[0].target.emulation, "显式选画像应生效");
    assert_eq!(targets[0].target.emulation_profile.as_deref(), Some("codex"));
    // 空串 = 显式关闭
    let ov_off: HashMap<String, String> = [("pi::p1::m1".to_string(), String::new())].into();
    let targets2 = expand_provider(&s1, &selected, 3, TestMode::NonStreaming, false, &ov_off);
    assert!(!targets2[0].target.emulation);
    // 无覆盖且供应商无 clientEmulation 配置 → 默认关
    let targets3 = expand_provider(&s1, &selected, 3, TestMode::NonStreaming, false, &HashMap::new());
    assert!(!targets3[0].target.emulation);
    // 供应商配置 enabled=true → 默认开（用配置的画像）
    let mut s2 = snapshot("p1", "A");
    s2.client_emulation = Some(ClientEmulationSpec {
        enabled: true,
        profile: "claude-code".into(),
        user_agent: None,
        headers: vec![],
    });
    let targets4 = expand_provider(&s2, &selected, 3, TestMode::NonStreaming, false, &HashMap::new());
    assert!(targets4[0].target.emulation);
    assert_eq!(targets4[0].target.emulation_profile.as_deref(), Some("claude-code"));
    // 显式选其他画像覆盖配置
    let ov_x: HashMap<String, String> = [("pi::p1::m1".to_string(), "gemini-cli".to_string())].into();
    let targets5 = expand_provider(&s2, &selected, 3, TestMode::NonStreaming, false, &ov_x);
    assert_eq!(targets5[0].target.emulation_profile.as_deref(), Some("gemini-cli"));
}

#[test]
fn cross_protocol_emulation_resolves() {
    // claude 供应商（anthropic_messages）显式选 codex 画像 → 应解析成功（跨协议由用户决定）
    let s1 = snapshot("p1", "A");
    let selected: HashSet<String> = ["m1".to_string()].into();
    let ov: HashMap<String, String> = [("pi::p1::m1".to_string(), "claude-code".to_string())].into();
    let targets = expand_provider(&s1, &selected, 3, TestMode::NonStreaming, false, &ov);
    let profile = cc_switch_model_tester_lib::emulation::resolve_profile(&targets[0].target).unwrap();
    assert_eq!(profile.name, "claude-code");
    // codex 画像用于 openai_chat（跨协议）也能解析
    let ov2: HashMap<String, String> = [("pi::p1::m1".to_string(), "codex".to_string())].into();
    let mut s3 = snapshot("p1", "A");
    s3.protocol = ApiProtocol::OpenAiChat;
    let targets2 = expand_provider(&s3, &selected, 3, TestMode::NonStreaming, false, &ov2);
    let p2 = cc_switch_model_tester_lib::emulation::resolve_profile(&targets2[0].target).unwrap();
    assert_eq!(p2.name, "codex");
}

#[test]
fn non_ready_providers_never_expand() {
    let mut s1 = snapshot("p1", "A");
    s1.status = ProviderStatus::ManagedAuthSkipped;
    assert!(expand(&s1, false).is_empty());
    let t: TestTarget = s1.to_test_target(&model_snap("m1"));
    let _ = t;
}
