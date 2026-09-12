// 与 Rust 端结构一一对应的类型定义（camelCase 由 serde rename 产生）

export interface AppErrorRepr {
  kind: string;
  message: string;
}

export interface AppInfo {
  name: string;
  version: string;
}

export interface AppTypeCount {
  appType: string;
  count: number;
}

export interface SourceInfo {
  path: string;
  exists: boolean;
  schemaVersion: number | null;
  providerCounts: AppTypeCount[];
  error: AppErrorRepr | null;
}

export type ProviderStatus = 'ready' | 'config_error' | 'managed_auth_skipped' | 'unsupported_protocol';

export interface ModelView {
  modelId: string;
  displayName: string | null;
  /** 客户端仿真的手动覆盖值（undefined = 跟随供应商配置默认值） */
  emulation?: boolean;
}

export interface ProviderCatalogView {
  id: string;
  appType: string;
  name: string;
  status: ProviderStatus;
  statusLabel: string;
  protocol: string; // snake_case，如 anthropic_messages
  rawProtocol: string | null;
  endpointDisplay: string;
  candidateEndpointCount: number;
  models: ModelView[];
  credentialLabel: string;
  /** 客户端仿真配置（pi 供应商；null = 未配置） */
  clientEmulation: { enabled: boolean; profile: string } | null;
  warnings: string[];
  error: string | null;
  rawConfigHash: string;
}

/** 选中状态 key：app::providerId::modelId（带应用前缀，避免跨标签页同名 provider/model 撞车） */
export function selectionKey(app: string, providerId: string, modelId: string): string {
  return `${app}::${providerId}::${modelId}`;
}

export type TabAppType = 'claude' | 'codex' | 'pi';

// ==================== M5：设置 / 历史 ====================

export interface PromptItem {
  id: string;
  name: string;
  content: string;
  enabled: boolean;
  isBuiltin: boolean;
}

export interface RuleItem {
  id: string;
  pattern: string;
  enabled: boolean;
  isBuiltin: boolean;
}

export interface AppSettingsView {
  /** 用户自定义代理；null = 未设置（用默认） */
  proxyUrl: string | null;
  defaultProxy: string;
  /** 用户自定义 cc-switch 数据库路径；null = 默认 */
  ccswitchDbPath: string | null;
  defaultCcswitchPath: string;
}

export interface HistoryRun {
  runId: string;
  startedAt: number;
  finishedAt: number | null;
  status: string;
  appType: string;
  totalAttempts: number;
  completedAttempts: number;
  passed: number;
  failed: number;
}

/** 历史明细行（后端 HistoryAttemptRow，驼峰） */
export interface HistoryAttempt {
  runId: string;
  appType: string;
  providerId: string;
  providerName: string;
  modelId: string;
  modelDisplayName: string | null;
  protocol: string;
  mode: string;
  endpointDisplay: string | null;
  attemptNo: number;
  promptText: string;
  testedAt: number;
  status: 'success' | 'failed';
  category: string;
  httpStatus: number | null;
  firstByteMs: number | null;
  totalLatencyMs: number | null;
  responseChars: number;
  responseSummary: string | null;
  errorSummary: string | null;
  matchedRule: string | null;
  /** 凭据末尾 4 位提示（如 …abcd），不含明文 */
  credentialHint: string | null;
  duplicateSourceCount: number;
}

export const TABS: { key: TabAppType; label: string }[] = [
  { key: 'claude', label: 'Claude Code' },
  { key: 'codex', label: 'Codex' },
  { key: 'pi', label: 'Pi agent' },
];

/** 把 invoke 抛出的错误统一转成可读文本 */
export function errText(e: unknown): string {
  if (e && typeof e === 'object' && 'message' in e) {
    const m = e as { kind?: string; message?: string };
    return m.message ?? JSON.stringify(e);
  }
  return String(e);
}
