-- 工具自身数据库 schema（%LOCALAPPDATA%\CcSwitchModelTester\data.db）
-- 按 DevelopmentPlan.md 第 13 节定义，M1 一次性建齐全部表。

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS prompt_templates (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  is_builtin INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS negative_rules (
  id TEXT PRIMARY KEY,
  pattern TEXT NOT NULL,
  match_type TEXT NOT NULL,
  scope TEXT NOT NULL,
  case_sensitive INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS test_runs (
  run_id TEXT PRIMARY KEY,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  status TEXT NOT NULL,
  total_attempts INTEGER NOT NULL,
  completed_attempts INTEGER NOT NULL DEFAULT 0,
  original_target_count INTEGER NOT NULL,
  deduplicated_target_count INTEGER NOT NULL,
  original_attempt_count INTEGER NOT NULL,
  deduplicated_attempt_count INTEGER NOT NULL,
  duplicate_group_count INTEGER NOT NULL DEFAULT 0,
  removed_attempt_count INTEGER NOT NULL DEFAULT 0,
  dedup_summary_json TEXT NOT NULL DEFAULT '{}',
  config_snapshot_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS test_attempts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  app_type TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  provider_name TEXT NOT NULL,
  model_id TEXT NOT NULL,
  model_display_name TEXT,
  protocol TEXT NOT NULL,
  mode TEXT NOT NULL,
  endpoint_url TEXT NOT NULL,
  endpoint_url_hash TEXT NOT NULL,
  dedup_key_hash TEXT NOT NULL,
  duplicate_source_count INTEGER NOT NULL DEFAULT 1,
  source_refs_json TEXT NOT NULL DEFAULT '[]',
  attempt_no INTEGER NOT NULL,
  prompt_text TEXT NOT NULL,
  tested_at INTEGER NOT NULL,
  status TEXT NOT NULL,
  category TEXT NOT NULL,
  http_status INTEGER,
  first_byte_ms INTEGER,
  first_text_ms INTEGER,
  total_latency_ms INTEGER,
  response_chars INTEGER NOT NULL DEFAULT 0,
  response_summary TEXT,
  error_summary TEXT,
  matched_rule TEXT,
  credential_hint TEXT,
  endpoint_display TEXT,
  FOREIGN KEY (run_id) REFERENCES test_runs(run_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_attempts_run ON test_attempts(run_id);
CREATE INDEX IF NOT EXISTS idx_attempts_tested_at ON test_attempts(tested_at);
