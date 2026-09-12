//! 测试调度器（DevelopmentPlan.md 第 10 节）。
//!
//! - 两级并发：全局信号量 + 供应商信号量（获取顺序按文档 10.2）
//! - 暂停：只阻止新任务开始；取消：中止正在进行的 HTTP 请求
//! - 失败不自动重试；被合并来源不产生虚假 attempt
//! - 事件推送（文档 14.2）：attempt-finished / run-progress / run-finished

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tauri::Emitter;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::dedup::{DedupGroup, TargetSourceRef, TestRunInput};
use crate::domain::{TestMode, TestTarget};
use crate::http::{ExecutionOutput, HttpClient, RequestLimits};
use crate::judge::{self, AttemptCategory, AttemptStatus, CompiledRule};
use crate::prompts::{self, PromptItem};
use crate::protocol::DEFAULT_MAX_TOKENS;
use crate::redact;
use crate::store;

// ==================== 运行状态 ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Paused,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}

pub struct RunHandle {
    pub cancel: CancellationToken,
    pub paused: AtomicBool,
    pub status: Mutex<RunStatus>,
    pub progress: Mutex<(usize, usize)>,
}

impl RunHandle {
    pub fn new(total: usize) -> Self {
        Self {
            cancel: CancellationToken::new(),
            paused: AtomicBool::new(false),
            status: Mutex::new(RunStatus::Running),
            progress: Mutex::new((0, total)),
        }
    }
}

/// 预览数据（内存，TTL 5 分钟；配置变化时失效）。
pub struct PreviewEntry {
    pub created: Instant,
    pub config_hashes: HashMap<String, String>,
    pub input: TestRunInput,
    pub groups: Vec<DedupGroup>,
    pub summary: crate::dedup::DedupSummary,
}

pub const PREVIEW_TTL_SECONDS: u64 = 300;

// ==================== 事件视图（脱敏） ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptFinishedView {
    pub run_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub endpoint_display: String,
    pub protocol: crate::domain::ApiProtocol,
    pub attempt_no: u32,
    pub status: AttemptStatus,
    pub category: AttemptCategory,
    pub http_status: Option<u16>,
    pub first_byte_ms: Option<u64>,
    pub total_ms: u64,
    pub response_chars: usize,
    pub response_summary: Option<String>,
    pub error_summary: Option<String>,
    pub matched_rule: Option<String>,
    pub prompt_text: String,
    /// 凭据末尾 4 位提示（如 …abcd），用于区分多 key；不含明文
    pub credential_hint: Option<String>,
    pub duplicate_source_count: usize,
    pub source_refs: Vec<TargetSourceRef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummaryView {
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub endpoint_display: String,
    pub protocol: crate::domain::ApiProtocol,
    pub attempts_sent: usize,
    pub success_count: usize,
    /// stable / unstable / unavailable / incomplete
    pub stability: String,
    pub success_rate: Option<f64>,
    pub avg_total_ms: Option<u64>,
    pub avg_first_byte_ms: Option<u64>,
    pub duplicate_source_count: usize,
    pub source_refs: Vec<TargetSourceRef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedView {
    pub run_id: String,
    pub status: RunStatus,
    pub summaries: Vec<ModelSummaryView>,
}

// ==================== 单次尝试执行 ====================

struct AttemptRecord {
    verdict: judge::AttemptVerdict,
    first_byte_ms: Option<u64>,
    total_ms: u64,
    cancelled: bool,
}

impl AttemptRecord {
    fn cancelled_record() -> Self {
        Self {
            verdict: judge::AttemptVerdict {
                status: AttemptStatus::Failed,
                category: AttemptCategory::Cancelled,
                http_status: None,
                matched_rule: None,
                message: "用户取消".to_string(),
                response_summary: None,
                error_summary: None,
                finish_signal: false,
                has_text: false,
            },
            first_byte_ms: None,
            total_ms: 0,
            cancelled: true,
        }
    }
}

async fn wait_if_paused(handle: &RunHandle) {
    while handle.paused.load(Ordering::Relaxed) && !handle.cancel.is_cancelled() {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
}

async fn execute_attempt(
    client: &HttpClient,
    limits: &RequestLimits,
    target: &TestTarget,
    mode: TestMode,
    prompt: &str,
    cancel: &CancellationToken,
    rules: &[CompiledRule],
    session_id: &str,
    apply_body_overrides: bool,
) -> AttemptRecord {
    let mut prepared = match crate::protocol::build_request(target, prompt, mode, DEFAULT_MAX_TOKENS) {
        Ok(p) => p,
        Err(e) => {
            return AttemptRecord {
                verdict: judge::AttemptVerdict {
                    status: AttemptStatus::Failed,
                    category: AttemptCategory::ConfigurationError,
                    http_status: None,
                    matched_rule: None,
                    message: format!("请求构造失败: {e}"),
                    response_summary: None,
                    error_summary: Some(redact::redact_text(&e)),
                    finish_signal: false,
                    has_text: false,
                },
                first_byte_ms: None,
                total_ms: 0,
                cancelled: false,
            };
        }
    };
    // ---- 出站改写层：① 客户端仿真 → ② cc-switch overrides.body 深度合并
    if let Some(profile) = crate::emulation::resolve_profile(target) {
        crate::emulation::apply_client_emulation(&mut prepared, target, profile, session_id);
    }
    if apply_body_overrides {
        if let Some(patch) = &target.local_proxy_body_patch {
            crate::emulation::apply_local_proxy_body_patch(&mut prepared.body, patch);
        }
    }
    let adapter = crate::protocol::adapter_for(target.protocol);
    let exec: ExecutionOutput = tokio::select! {
        r = client.execute(&prepared, adapter.as_ref(), mode, limits) => r,
        _ = cancel.cancelled() => ExecutionOutput {
            network_error: Some("用户取消".to_string()),
            ..Default::default()
        },
    };
    let cancelled = exec.network_error.as_deref() == Some("用户取消");
    let verdict = judge::judge(&exec, rules);
    AttemptRecord {
        verdict,
        first_byte_ms: exec.first_byte_ms,
        total_ms: exec.total_ms,
        cancelled,
    }
}

// ==================== 历史写入 ====================

fn write_attempt(
    run_id: &str,
    group: &DedupGroup,
    mode: TestMode,
    attempt_no: u32,
    prompt: &str,
    rec: &AttemptRecord,
    credential_hint: Option<&str>,
) -> Result<(), String> {
    let conn = rusqlite::Connection::open(store::db_path())
        .map_err(|e| format!("打开本地库失败: {e}"))?;
    let v = &rec.verdict;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let t = &group.representative;
    conn.execute(
        "INSERT INTO test_attempts (
            run_id, app_type, provider_id, provider_name, model_id, model_display_name,
            protocol, mode, endpoint_url, endpoint_url_hash, dedup_key_hash,
            duplicate_source_count, source_refs_json, attempt_no, prompt_text, tested_at,
            status, category, http_status, first_byte_ms, total_latency_ms,
            response_chars, response_summary, error_summary, matched_rule, credential_hint, endpoint_display
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27)",
        rusqlite::params![
            run_id,
            t.app.as_str(),
            t.provider_id,
            t.provider_name,
            t.model_id,
            t.model_display_name,
            t.protocol.db_str(),
            match mode {
                TestMode::NonStreaming => "non_streaming",
                TestMode::Streaming => "streaming",
            },
            t.endpoint_url,
            redact::redact_url(&t.endpoint_url),
            group.key_hash,
            group.merged_sources.len() as i64,
            serde_json::to_string(&group.merged_sources).unwrap_or_default(),
            attempt_no,
            prompt,
            now,
            format!("{:?}", v.status).to_lowercase(),
            format!("{:?}", v.category).to_lowercase(),
            v.http_status.map(|s| s as i64),
            rec.first_byte_ms.map(|m| m as i64),
            rec.total_ms as i64,
            v.response_summary.as_ref().map(|s| s.chars().count()).unwrap_or(0) as i64,
            v.response_summary,
            v.error_summary,
            v.matched_rule,
            credential_hint,
            redact::redact_url(&t.endpoint_url),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ==================== 主任务 ====================

#[allow(clippy::too_many_arguments)]
pub async fn run_task(
    app: tauri::AppHandle,
    run_id: String,
    input: TestRunInput,
    groups: Vec<DedupGroup>,
    handle: Arc<RunHandle>,
) {
    let mode = input.mode;
    let attempts_per_model = input.attempts_per_model;
    let provider_concurrency = input.provider_concurrency.max(1) as usize;
    let total = groups.len() * attempts_per_model as usize;

    // ---- 提示词 / 规则 / 代理 / 客户端 ----
    let prompts_list: Vec<PromptItem> = match rusqlite::Connection::open(store::db_path()) {
        Ok(conn) => prompts::load_enabled_prompts(&conn).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    if prompts_list.is_empty() {
        set_failed(&app, &run_id, &handle, "没有已启用的测试提示词");
        return;
    }
    let rules: Vec<CompiledRule> = match rusqlite::Connection::open(store::db_path()) {
        Ok(conn) => judge::load_compiled_rules(&conn).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let proxy = match rusqlite::Connection::open(store::db_path()) {
        Ok(conn) => store::get_setting(&conn, "proxy_url")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            // 需求文档 1.5：默认代理 socks5://127.0.0.1:1080；设置里显式填“direct”可强制直连
            .or_else(|| Some(store::DEFAULT_PROXY.to_string())),
        Err(_) => Some(store::DEFAULT_PROXY.to_string()),
    };
    // 超时可由用户设置（问题反馈：默认 180s 太长，单个请求卡住会阻塞同供应商后续测试）
    let timeout_secs = input.timeout_seconds.clamp(5, 600) as u64;
    let limits = RequestLimits {
        connect_timeout: std::time::Duration::from_secs(timeout_secs.min(15)),
        total_timeout: std::time::Duration::from_secs(timeout_secs),
        idle_timeout: std::time::Duration::from_secs((timeout_secs / 2).clamp(10, 60)),
    };
    let client = match HttpClient::new(proxy.as_deref(), &limits) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            set_failed(&app, &run_id, &handle, &format!("HTTP 客户端初始化失败: {e}"));
            return;
        }
    };

    // ---- 并发结构 ----
    let global_sem = Arc::new(Semaphore::new(input.global_concurrency.max(1) as usize));
    let provider_sems: Arc<Mutex<HashMap<String, Arc<Semaphore>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let completed = Arc::new(AtomicUsize::new(0));
    // 每个去重组上次使用的提示词 id（实现"同模型连续不重复"）
    let last_prompt: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let prompts_arc = Arc::new(prompts_list);
    let rules = Arc::new(rules);

    let arc_groups: Vec<Arc<DedupGroup>> = groups.into_iter().map(Arc::new).collect();

    // ---- 逐尝试任务 ----
    let mut jobs = Vec::with_capacity(total);
    for group in &arc_groups {
        let group_key = group.key_hash.clone();
        for attempt_no in 1..=attempts_per_model {
            let app = app.clone();
            let run_id = run_id.clone();
            let group = group.clone();
            let group_key = group_key.clone();
            let handle = handle.clone();
            let cancel = handle.cancel.clone();
            let global_sem = global_sem.clone();
            let provider_sems = provider_sems.clone();
            let completed = completed.clone();
            let last_prompt = last_prompt.clone();
            let prompts_arc = prompts_arc.clone();
            let rules = rules.clone();
            let client = client.clone();
            let limits = limits.clone();
            jobs.push(tauri::async_runtime::spawn(async move {
                // 1. 暂停等待（暂停不阻在途请求）
                wait_if_paused(&handle).await;
                if cancel.is_cancelled() {
                    return AttemptRecord::cancelled_record();
                }
                // 2. 两级信号量（全局 → 供应商）
                let _global_permit = global_sem.acquire().await;
                let provider_permit = {
                    let mut map = provider_sems.lock().unwrap();
                    map.entry(group.representative.provider_id.clone())
                        .or_insert_with(|| Arc::new(Semaphore::new(provider_concurrency)))
                        .clone()
                };
                // provider 并发度由创建信号量时的设置决定，此处取出
                let _provider_permit = provider_permit.acquire().await;
                if cancel.is_cancelled() {
                    return AttemptRecord::cancelled_record();
                }
                // 3. 随机提示词（同组连续不重复）
                let prompt = {
                    let mut r = rand::rng();
                    let last = last_prompt.lock().unwrap().get(&group_key).cloned();
                    match prompts::pick_random(&prompts_arc, last.as_deref(), &mut r) {
                        Some(p) => {
                            last_prompt.lock().unwrap().insert(group_key, p.id.clone());
                            p.content
                        }
                        None => String::new(),
                    }
                };
                // 4. 执行（可被取消中断）；session_id = run_id（一轮一个，与 pi 插件同语义）
                tracing::info!(
                    emulation = group.representative.emulation,
                    profile = ?group.representative.emulation_profile,
                    resolved = ?crate::emulation::resolve_profile(&group.representative).map(|p| p.name),
                    "[诊断] execute_attempt 入口仿真状态"
                );
                let rec = execute_attempt(
                    &client,
                    &limits,
                    &group.representative,
                    mode,
                    &prompt,
                    &cancel,
                    &rules,
                    &run_id,
                    input.apply_body_overrides,
                )
                .await;
                drop(_provider_permit);
                drop(_global_permit);
                // 凭据末尾提示（区分多 key；不含明文）
                let cred_hint = group
                    .representative
                    .credential
                    .as_ref()
                    .and_then(|c| redact::credential_hint(&c.secret));
                // 5. 写历史
                if let Err(e) = write_attempt(
                    &run_id,
                    &group,
                    mode,
                    attempt_no,
                    &prompt,
                    &rec,
                    cred_hint.as_deref(),
                ) {
                    tracing::warn!("写入尝试记录失败: {e}");
                }
                // 6. 事件
                let v = &rec.verdict;
                let view = AttemptFinishedView {
                    run_id: run_id.clone(),
                    provider_id: group.representative.provider_id.clone(),
                    provider_name: group.representative.provider_name.clone(),
                    model_id: group.representative.model_id.clone(),
                    endpoint_display: redact::redact_url(&group.representative.endpoint_url),
                    protocol: group.representative.protocol,
                    attempt_no,
                    status: v.status,
                    category: v.category,
                    http_status: v.http_status,
                    first_byte_ms: rec.first_byte_ms,
                    total_ms: rec.total_ms,
                    response_chars: v.response_summary.as_ref().map(|s| s.chars().count()).unwrap_or(0),
                    response_summary: v.response_summary.clone(),
                    error_summary: v.error_summary.clone(),
                    matched_rule: v.matched_rule.clone(),
                    prompt_text: prompt.clone(),
                    credential_hint: cred_hint.clone(),
                    duplicate_source_count: group.merged_sources.len(),
                    source_refs: group.merged_sources.clone(),
                };
                let _ = app.emit("test-attempt-finished", &view);
                let c = completed.fetch_add(1, Ordering::Relaxed) + 1;
                *handle.progress.lock().unwrap() = (c, total);
                let _ = app.emit("test-run-progress", serde_json::json!({ "runId": run_id, "completed": c, "total": total }));
                rec
            }));
        }
    }

    // ---- 收集结果（保序：groups × attempts）----
    let mut results: Vec<AttemptRecord> = Vec::with_capacity(total);
    for j in jobs {
        match j.await {
            Ok(rec) => results.push(rec),
            Err(_) => results.push(AttemptRecord::cancelled_record()),
        }
    }

    let cancelled_run = handle.cancel.is_cancelled();

    // ---- 汇总 ----
    let mut summaries: Vec<ModelSummaryView> = Vec::new();
    for (gi, group) in arc_groups.iter().enumerate() {
        let records = &results[gi * attempts_per_model as usize..(gi + 1) * attempts_per_model as usize];
        summaries.push(summarize(group, records, cancelled_run));
    }

    let final_status = if cancelled_run { RunStatus::Cancelled } else { RunStatus::Completed };
    *handle.status.lock().unwrap() = final_status;

    // ---- 更新历史 ----
    if let Ok(conn) = rusqlite::Connection::open(store::db_path()) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let completed_n = completed.load(Ordering::Relaxed) as i64;
        let _ = conn.execute(
            "UPDATE test_runs SET finished_at = ?1, status = ?2, completed_attempts = ?3 WHERE run_id = ?4",
            rusqlite::params![now, format!("{:?}", final_status).to_lowercase(), completed_n, run_id],
        );
    }

    let view = RunFinishedView { run_id: run_id.clone(), status: final_status, summaries };
    let _ = app.emit("test-run-finished", &view);
    if cancelled_run {
        let _ = app.emit("test-run-cancelled", serde_json::json!({ "runId": run_id }));
    }
}

fn summarize(group: &DedupGroup, records: &[AttemptRecord], run_cancelled: bool) -> ModelSummaryView {
    let sent: Vec<&AttemptRecord> = records.iter().filter(|r| !r.cancelled).collect();
    let success = sent.iter().filter(|r| r.verdict.status == AttemptStatus::Success).count();
    let stability = if run_cancelled || sent.is_empty() {
        "incomplete"
    } else if success == sent.len() {
        "stable"
    } else if success == 0 {
        "unavailable"
    } else {
        "unstable"
    };
    let avg_total = if sent.is_empty() {
        None
    } else {
        Some(sent.iter().map(|r| r.total_ms).sum::<u64>() / sent.len() as u64)
    };
    let avg_fb = {
        let vals: Vec<u64> = sent.iter().filter_map(|r| r.first_byte_ms).collect();
        if vals.is_empty() { None } else { Some(vals.iter().sum::<u64>() / vals.len() as u64) }
    };
    let rate = if sent.is_empty() { None } else { Some(success as f64 / sent.len() as f64) };
    ModelSummaryView {
        provider_id: group.representative.provider_id.clone(),
        provider_name: group.representative.provider_name.clone(),
        model_id: group.representative.model_id.clone(),
        endpoint_display: redact::redact_url(&group.representative.endpoint_url),
        protocol: group.representative.protocol,
        attempts_sent: sent.len(),
        success_count: success,
        stability: stability.to_string(),
        success_rate: rate,
        avg_total_ms: avg_total,
        avg_first_byte_ms: avg_fb,
        duplicate_source_count: group.merged_sources.len(),
        source_refs: group.merged_sources.clone(),
    }
}

fn set_failed(app: &tauri::AppHandle, run_id: &str, handle: &RunHandle, msg: &str) {
    *handle.status.lock().unwrap() = RunStatus::Failed;
    handle.cancel.cancel();
    tracing::error!("运行 {run_id} 失败: {msg}");
    let _ = app.emit("test-run-finished", serde_json::json!({ "runId": run_id, "status": "failed", "error": msg, "summaries": [] }));
}
