//! 本地日志初始化。
//!
//! 日志写入 `%LOCALAPPDATA%\CcSwitchModelTester\logs\`，按天滚动。
//! 约定：任何包含凭据的内容必须先经过 `crate::redact` 才能进入日志。

use std::sync::OnceLock;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// 初始化日志系统。应在应用启动时调用一次。
pub fn init_logging() {
    let dir = crate::store::logs_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("创建日志目录失败: {e}");
        return;
    }
    let appender = tracing_appender::rolling::daily(&dir, "app.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // non-blocking writer 的 guard 必须全程保活，否则日志线程退出
    let _ = GUARD.set(guard);
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(writer)
        .try_init();
    tracing::info!("日志系统初始化完成，目录: {}", dir.display());
}
