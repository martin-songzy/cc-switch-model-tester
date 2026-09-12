//! cc-switch Model Tester — Tauri 应用入口。
//!
//! 所有 cc-switch 数据访问均为只读；settings_config 原文不离开 Rust。

pub mod ccswitch;
pub mod dedup;
pub mod emulation;
pub mod domain;
pub mod error;
pub mod http;
pub mod judge;
mod logging;
pub mod parser;
pub mod prompts;
pub mod protocol;
pub mod redact;
pub mod scheduler;
mod store;

/// 统一 User-Agent 基准值（customUserAgent 可覆盖）。
pub const APP_USER_AGENT: &str = concat!("cc-switch-model-tester/", env!("CARGO_PKG_VERSION"));

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rand::RngExt;
use rusqlite::Connection;

use ccswitch::db::{
    default_db_path, ensure_supported_schema, inspect_source, load_candidate_endpoints,
    load_provider_rows, open_read_only, read_schema_version, SourceInfo,
};
use domain::{AppType, ProviderCatalogView};
use error::{AppError, AppErrorKind};
use scheduler::{PreviewEntry, RunHandle, RunStatus, PREVIEW_TTL_SECONDS};
use tauri::{Emitter, State};

/// 全局应用状态：工具自身 SQLite + 运行句柄 + 预览缓存。
pub struct AppState {
    pub store: Mutex<Connection>,
    pub runs: Mutex<HashMap<String, Arc<RunHandle>>>,
    pub previews: Mutex<HashMap<String, PreviewEntry>>,
}

fn setting_conn<'a>(state: &'a State<AppState>) -> std::sync::MutexGuard<'a, Connection> {
    state.store.lock().expect("本地存储连接锁中毒")
}

/// 解析当前数据源路径：优先使用用户已保存的路径，否则用默认路径。
fn resolve_source_path(state: &State<AppState>) -> Result<std::path::PathBuf, AppError> {
    let saved = {
        let conn = setting_conn(state);
        store::get_setting(&conn, "ccswitch_db_path")?
    };
    if let Some(p) = saved {
        return Ok(std::path::PathBuf::from(p));
    }
    default_db_path().ok_or_else(|| {
        AppError::new(
            AppErrorKind::CcSwitchDbNotFound,
            "无法定位用户主目录，请在设置中手动选择 cc-switch.db 文件",
        )
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn gen_id(prefix: &str) -> String {
    let mut rng = rand::rng();
    let rnd: String = (0..8).map(|_| format!("{:x}", rng.random_range(0u8..16))).collect();
    format!("{prefix}-{}-{rnd}", chrono_like_ts())
}

fn chrono_like_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[tauri::command]
fn get_app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "cc-switch Model Tester",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// 探测当前数据源（默认路径或用户已保存路径）。
#[tauri::command]
fn get_ccswitch_source(state: State<AppState>) -> SourceInfo {
    match resolve_source_path(&state) {
        Ok(path) => inspect_source(&path),
        Err(e) => SourceInfo {
            path: String::new(),
            exists: false,
            schema_version: None,
            provider_counts: Vec::new(),
            error: Some(ccswitch::db::SourceError {
                kind: e.kind,
                message: e.message,
            }),
        },
    }
}

/// 校验并保存用户选择的数据源路径；空串 = 恢复默认（~/.cc-switch/cc-switch.db）。
#[tauri::command]
fn set_ccswitch_source(path: String, state: State<AppState>) -> Result<SourceInfo, AppError> {
    if path.trim().is_empty() {
        {
            let conn = setting_conn(&state);
            conn.execute("DELETE FROM app_settings WHERE key = 'ccswitch_db_path'", [])?;
        }
        tracing::info!("已恢复 cc-switch 数据源默认路径");
        return Ok(get_ccswitch_source(state));
    }
    let info = inspect_source(Path::new(&path));
    if let Some(err) = &info.error {
        return Err(AppError::new(err.kind, err.message.clone()));
    }
    {
        let conn = setting_conn(&state);
        store::set_setting(&conn, "ccswitch_db_path", &path)?;
    }
    tracing::info!("已保存 cc-switch 数据源路径: {}", redact::redact_text(&path));
    Ok(info)
}

/// 加载某个应用类型的完整供应商目录（含模型，脱敏后返回）。
#[tauri::command]
fn load_provider_catalog(
    app_type: String,
    state: State<AppState>,
) -> Result<Vec<ProviderCatalogView>, AppError> {
    let app = AppType::from_db_str(&app_type).ok_or_else(|| {
        AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            format!("不支持的应用类型: {app_type}"),
        )
    })?;
    let path = resolve_source_path(&state)?;
    let conn = open_read_only(&path)?;
    let version = read_schema_version(&conn)?;
    ensure_supported_schema(version)?;
    let rows = load_provider_rows(&conn, &app_type)?;
    let candidates = load_candidate_endpoints(&conn, &app_type)?;
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let cand = candidates.get(&row.id).cloned().unwrap_or_default();
        let snap =
            parser::parse_provider(app, &row.id, &row.name, &row.settings_config, &row.meta, cand);
        views.push(snap.to_catalog_view());
    }
    tracing::debug!(app_type, count = views.len(), "供应商目录加载完成");
    Ok(views)
}

/// 预览测试：展开目标 + 精确去重（不发请求、不写历史，文档 14.1）。
#[tauri::command]
fn preview_test(
    input: dedup::TestRunInput,
    state: State<AppState>,
) -> Result<dedup::DedupPreview, AppError> {
    // 前置校验
    {
        let conn = setting_conn(&state);
        prompts::ensure_any_enabled(&conn)?;
    }
    if input.attempts_per_model == 0 || input.attempts_per_model > 20 {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            "测试次数须在 1~20 之间",
        ));
    }
    if input.global_concurrency == 0 || input.global_concurrency > 50 {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            "全局并发须在 1~50 之间",
        ));
    }
    if input.provider_concurrency == 0 || input.provider_concurrency > 10 {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            "单供应商并发须在 1~10 之间",
        ));
    }
    if input.provider_ids.is_empty() {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            "没有选中任何供应商",
        ));
    }

    let path = resolve_source_path(&state)?;
    let conn = open_read_only(&path)?;
    ensure_supported_schema(read_schema_version(&conn)?)?;
    let rows = load_provider_rows(&conn, input.app.as_str())?;
    let candidates = load_candidate_endpoints(&conn, input.app.as_str())?;

    // 解析 + 展开
    let mut expanded: Vec<dedup::ExpandedTarget> = Vec::new();
    let mut hashes: HashMap<String, String> = HashMap::new();
    let mut skipped: Vec<dedup::SkippedProviderView> = Vec::new();
    for row in rows {
        if !input.provider_ids.contains(&row.id) {
            continue;
        }
        let snap = parser::parse_provider(
            input.app,
            &row.id,
            &row.name,
            &row.settings_config,
            &row.meta,
            candidates.get(&row.id).cloned().unwrap_or_default(),
        );
        hashes.insert(row.id.clone(), snap.raw_config_hash.clone());
        if snap.status != domain::ProviderStatus::Ready {
            skipped.push(dedup::SkippedProviderView {
                provider_id: row.id.clone(),
                provider_name: row.name.clone(),
                reason: snap
                    .config_error
                    .unwrap_or_else(|| snap.status.ui_label().to_string()),
            });
            continue;
        }
        let selected = dedup::selected_models_for(input.app.as_str(), &row.id, &input.model_keys);
        expanded.extend(dedup::expand_provider(
            &snap,
            &selected,
            input.attempts_per_model,
            input.mode,
            input.test_all_candidate_endpoints,
            &input.emulation_overrides,
        ));
    }
    if expanded.is_empty() {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            "没有可测试的目标：所选供应商均不可测试或未选中任何模型",
        ));
    }

    let (groups, summary) = dedup::deduplicate(expanded, input.mode, input.attempts_per_model);
    let preview_id = gen_id("preview");
    let expires_at = now_unix() + PREVIEW_TTL_SECONDS as i64;
    let preview =
        dedup::build_preview(preview_id.clone(), &groups, &summary, input.mode, input.attempts_per_model, skipped, expires_at);

    state.previews.lock().unwrap().insert(
        preview_id,
        PreviewEntry {
            created: Instant::now(),
            config_hashes: hashes,
            input,
            groups,
            summary,
        },
    );
    Ok(preview)
}

/// 启动测试批次：校验预览有效性与配置快照，然后进入调度器。
#[tauri::command]
fn start_test(
    preview_id: String,
    state: State<AppState>,
    app: tauri::AppHandle,
) -> Result<String, AppError> {
    let entry = {
        let mut previews = state.previews.lock().unwrap();
        previews.remove(&preview_id)
    }
    .ok_or_else(|| {
        AppError::new(AppErrorKind::Internal, "预览不存在或已使用，请重新预览")
    })?;
    if entry.created.elapsed() > std::time::Duration::from_secs(PREVIEW_TTL_SECONDS) {
        return Err(AppError::new(AppErrorKind::Internal, "预览已过期，请重新预览"));
    }

    // 配置快照校验：任一供应商 settings_config 变化 → 要求重新预览（文档 14.1）
    {
        let path = resolve_source_path(&state)?;
        let conn = open_read_only(&path)?;
        let rows = load_provider_rows(&conn, entry.input.app.as_str())?;
        let candidates = load_candidate_endpoints(&conn, entry.input.app.as_str())?;
        for row in &rows {
            if !entry.config_hashes.contains_key(&row.id) {
                continue;
            }
            let snap = parser::parse_provider(
                entry.input.app,
                &row.id,
                &row.name,
                &row.settings_config,
                &row.meta,
                candidates.get(&row.id).cloned().unwrap_or_default(),
            );
            if snap.raw_config_hash != entry.config_hashes[&row.id] {
                return Err(AppError::new(
                    AppErrorKind::Internal,
                    "preview_expired_or_changed：cc-switch 配置已变化，请重新预览",
                ));
            }
        }
    }

    let run_id = gen_id("run");
    let total = entry.groups.len() * entry.input.attempts_per_model as usize;
    {
        let conn = setting_conn(&state);
        conn.execute(
            "INSERT INTO test_runs (run_id, started_at, status, total_attempts, completed_attempts,
                original_target_count, deduplicated_target_count, original_attempt_count,
                deduplicated_attempt_count, duplicate_group_count, removed_attempt_count,
                dedup_summary_json, config_snapshot_hash, created_at)
             VALUES (?1, ?2, 'running', ?3, 0, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?2)",
            rusqlite::params![
                run_id,
                now_unix(),
                total as i64,
                entry.summary.original_target_count as i64,
                entry.summary.deduplicated_target_count as i64,
                entry.summary.original_attempt_count as i64,
                entry.summary.deduplicated_attempt_count as i64,
                entry.summary.duplicate_group_count as i64,
                entry.summary.removed_attempt_count as i64,
                serde_json::to_string(&entry.summary).unwrap_or_default(),
                format!("groups:{}", entry.groups.len()),
            ],
        )?;
        // 自动只保留最近 15 分钟内的轮次（用户设定），避免数据库无限增长
        store::prune_history_before(&conn, now_unix() - store::HISTORY_KEEP_SECONDS)?;
    }

    let handle = Arc::new(RunHandle::new(total));
    state.runs.lock().unwrap().insert(run_id.clone(), handle.clone());

    // test-run-created 事件必须包含去重摘要（文档 14.2）
    let _ = app.emit(
        "test-run-created",
        serde_json::json!({
            "runId": run_id,
            "originalTargetCount": entry.summary.original_target_count,
            "deduplicatedTargetCount": entry.summary.deduplicated_target_count,
            "originalAttemptCount": entry.summary.original_attempt_count,
            "deduplicatedAttemptCount": entry.summary.deduplicated_attempt_count,
            "duplicateGroupCount": entry.summary.duplicate_group_count,
            "removedAttemptCount": entry.summary.removed_attempt_count,
            "totalAttempts": total,
        }),
    );

    tauri::async_runtime::spawn(scheduler::run_task(
        app.clone(),
        run_id.clone(),
        entry.input,
        entry.groups,
        handle,
    ));
    tracing::info!(%run_id, total, "测试批次已启动");
    Ok(run_id)
}

#[tauri::command]
fn pause_test(run_id: String, state: State<AppState>) -> Result<(), AppError> {
    let h = state
        .runs
        .lock()
        .unwrap()
        .get(&run_id)
        .cloned()
        .ok_or_else(|| AppError::new(AppErrorKind::Internal, "运行不存在"))?;
    h.paused.store(true, std::sync::atomic::Ordering::Relaxed);
    *h.status.lock().unwrap() = RunStatus::Paused;
    Ok(())
}

#[tauri::command]
fn resume_test(run_id: String, state: State<AppState>) -> Result<(), AppError> {
    let h = state
        .runs
        .lock()
        .unwrap()
        .get(&run_id)
        .cloned()
        .ok_or_else(|| AppError::new(AppErrorKind::Internal, "运行不存在"))?;
    h.paused
        .store(false, std::sync::atomic::Ordering::Relaxed);
    if *h.status.lock().unwrap() == RunStatus::Paused {
        *h.status.lock().unwrap() = RunStatus::Running;
    }
    Ok(())
}

#[tauri::command]
fn cancel_test(run_id: String, state: State<AppState>) -> Result<(), AppError> {
    let h = state
        .runs
        .lock()
        .unwrap()
        .get(&run_id)
        .cloned()
        .ok_or_else(|| AppError::new(AppErrorKind::Internal, "运行不存在"))?;
    h.cancel.cancel();
    *h.status.lock().unwrap() = RunStatus::Cancelling;
    Ok(())
}

#[tauri::command]
fn get_run_status(run_id: String, state: State<AppState>) -> Result<serde_json::Value, AppError> {
    let h = state
        .runs
        .lock()
        .unwrap()
        .get(&run_id)
        .cloned()
        .ok_or_else(|| AppError::new(AppErrorKind::Internal, "运行不存在"))?;
    let (completed, total) = *h.progress.lock().unwrap();
    Ok(serde_json::json!({
        "runId": run_id,
        "status": *h.status.lock().unwrap(),
        "completed": completed,
        "total": total,
    }))
}

// ==================== M5：设置（数据源 / 代理 / 提示词 / 负面规则） ====================

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSettingsView {
    /// 用户自定义代理；None = 未设置（使用默认 socks5://127.0.0.1:1080）
    proxy_url: Option<String>,
    default_proxy: String,
    /// 用户自定义 cc-switch 数据库路径；None = 使用默认 ~/.cc-switch/cc-switch.db
    ccswitch_db_path: Option<String>,
    default_ccswitch_path: String,
}

#[tauri::command]
fn get_app_settings(state: State<AppState>) -> Result<AppSettingsView, AppError> {
    let conn = setting_conn(&state);
    Ok(AppSettingsView {
        proxy_url: store::get_setting(&conn, "proxy_url")?,
        default_proxy: store::DEFAULT_PROXY.to_string(),
        ccswitch_db_path: store::get_setting(&conn, "ccswitch_db_path")?,
        default_ccswitch_path: default_db_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    })
}

/// 保存代理地址；空串/空白 = 恢复默认（socks5://127.0.0.1:1080）。
#[tauri::command]
fn set_proxy_url(url: String, state: State<AppState>) -> Result<(), AppError> {
    let conn = setting_conn(&state);
    let url = url.trim().to_string();
    if url.is_empty() {
        conn.execute("DELETE FROM app_settings WHERE key = 'proxy_url'", [])?;
    } else {
        store::set_setting(&conn, "proxy_url", &url)?;
    }
    tracing::info!("已更新代理设置");
    Ok(())
}

#[tauri::command]
fn list_prompts(state: State<AppState>) -> Result<Vec<store::PromptRow>, AppError> {
    let conn = setting_conn(&state);
    store::list_prompts(&conn)
}

#[tauri::command]
fn save_prompt(
    id: Option<String>,
    name: String,
    content: String,
    enabled: bool,
    state: State<AppState>,
) -> Result<String, AppError> {
    let conn = setting_conn(&state);
    let saved = store::save_prompt(&conn, id.as_deref(), &name, &content, enabled)?;
    tracing::info!("已保存提示词 {saved}");
    Ok(saved)
}

#[tauri::command]
fn delete_prompt(id: String, state: State<AppState>) -> Result<(), AppError> {
    let conn = setting_conn(&state);
    store::delete_prompt(&conn, &id)
}

#[tauri::command]
fn list_rules(state: State<AppState>) -> Result<Vec<store::RuleRow>, AppError> {
    let conn = setting_conn(&state);
    store::list_rules(&conn)
}

#[tauri::command]
fn save_rule(
    id: Option<String>,
    pattern: String,
    enabled: bool,
    state: State<AppState>,
) -> Result<String, AppError> {
    let conn = setting_conn(&state);
    let saved = store::save_rule(&conn, id.as_deref(), &pattern, enabled)?;
    tracing::info!("已保存负面规则 {saved}");
    Ok(saved)
}

#[tauri::command]
fn delete_rule(id: String, state: State<AppState>) -> Result<(), AppError> {
    let conn = setting_conn(&state);
    store::delete_rule(&conn, &id)
}

// ==================== M5：测试历史 ====================

#[tauri::command]
fn history_runs(state: State<AppState>) -> Result<Vec<store::HistoryRunRow>, AppError> {
    let conn = setting_conn(&state);
    store::history_runs(&conn, now_unix())
}

#[tauri::command]
fn history_attempts(
    run_id: String,
    state: State<AppState>,
) -> Result<Vec<store::HistoryAttemptRow>, AppError> {
    let conn = setting_conn(&state);
    store::history_attempts(&conn, &run_id)
}

#[tauri::command]
fn history_delete_run(run_id: String, state: State<AppState>) -> Result<(), AppError> {
    let conn = setting_conn(&state);
    store::delete_history_run(&conn, &run_id)
}

#[tauri::command]
fn history_clear(state: State<AppState>) -> Result<(), AppError> {
    let conn = setting_conn(&state);
    store::clear_history(&conn)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init_logging();
    let store = store::init_store().expect("初始化本地存储失败");
    tracing::info!(
        "应用启动 v{}，本地数据目录: {}",
        env!("CARGO_PKG_VERSION"),
        store::data_dir().display()
    );
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            store: Mutex::new(store),
            runs: Mutex::new(HashMap::new()),
            previews: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            get_ccswitch_source,
            set_ccswitch_source,
            load_provider_catalog,
            preview_test,
            start_test,
            pause_test,
            resume_test,
            cancel_test,
            get_run_status,
            get_app_settings,
            set_proxy_url,
            list_prompts,
            save_prompt,
            delete_prompt,
            list_rules,
            save_rule,
            delete_rule,
            history_runs,
            history_attempts,
            history_delete_run,
            history_clear
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
