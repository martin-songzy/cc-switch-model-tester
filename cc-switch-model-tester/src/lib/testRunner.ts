// 测试执行 API + 事件监听封装
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { errText, selectionKey, type TabAppType, type ProviderCatalogView } from './types';

export type TestMode = 'non_streaming' | 'streaming';

export interface TestRunInput {
  app: TabAppType;
  providerIds: string[];
  modelKeys: string[]; // app::providerId::modelId
  attemptsPerModel: number;
  mode: TestMode;
  globalConcurrency: number;
  providerConcurrency: number;
  testAllCandidateEndpoints: boolean;
  applyBodyOverrides: boolean;
  /** 每模型客户端仿真开关（key = selectionKey）；缺省跟随供应商配置默认值 */
  emulationOverrides: Record<string, boolean>;
  timeoutSeconds: number;
}

export interface SkippedProvider {
  providerId: string;
  providerName: string;
  reason: string;
}

export interface TargetSourceRef {
  app: string;
  providerId: string;
  providerName: string;
  modelKey: string;
}

export interface DedupGroupView {
  keyHash: string;
  representative: {
    app: string;
    providerId: string;
    providerName: string;
    modelId: string;
    endpointDisplay: string;
    protocol: string;
    mode: string;
    /** 客户端仿真（开启时为画像名，如 claude-code） */
    emulationProfile?: string;
  };
  mergedSources: TargetSourceRef[];
  removedTargetCount: number;
  removedAttemptCount: number;
}

export interface DedupPreview {
  previewId: string;
  summary: {
    originalTargetCount: number;
    deduplicatedTargetCount: number;
    originalAttemptCount: number;
    deduplicatedAttemptCount: number;
    duplicateGroupCount: number;
    removedAttemptCount: number;
  };
  groups: DedupGroupView[];
  skipped: SkippedProvider[];
  expiresAt: number;
}

export interface AttemptFinished {
  runId: string;
  providerId: string;
  providerName: string;
  modelId: string;
  endpointDisplay: string;
  protocol: string;
  attemptNo: number;
  status: 'success' | 'failed';
  category: string;
  httpStatus: number | null;
  firstByteMs: number | null;
  totalMs: number;
  responseChars: number;
  responseSummary: string | null;
  errorSummary: string | null;
  matchedRule: string | null;
  /** 凭据末尾 4 位提示（如 …abcd），不含明文 */
  credentialHint: string | null;
  promptText: string;
  duplicateSourceCount: number;
  sourceRefs: TargetSourceRef[];
}

export interface ModelSummary {
  providerId: string;
  providerName: string;
  modelId: string;
  endpointDisplay: string;
  protocol: string;
  attemptsSent: number;
  successCount: number;
  stability: 'stable' | 'unstable' | 'unavailable' | 'incomplete';
  successRate: number | null;
  avgTotalMs: number | null;
  avgFirstByteMs: number | null;
  duplicateSourceCount: number;
  sourceRefs: TargetSourceRef[];
}

export interface RunFinished {
  runId: string;
  status: string;
  summaries: ModelSummary[];
}

export function previewTest(input: TestRunInput): Promise<DedupPreview> {
  return invoke('preview_test', { input });
}

export function startTest(previewId: string): Promise<string> {
  return invoke('start_test', { previewId });
}

export function pauseTest(runId: string): Promise<void> {
  return invoke('pause_test', { runId });
}

export function resumeTest(runId: string): Promise<void> {
  return invoke('resume_test', { runId });
}

export function cancelTest(runId: string): Promise<void> {
  return invoke('cancel_test', { runId });
}

export function getRunStatus(runId: string): Promise<{ runId: string; status: string; completed: number; total: number }> {
  return invoke('get_run_status', { runId });
}

/** 订阅运行事件，返回解绑函数 */
export async function listenRunEvents(handlers: {
  onAttemptFinished?: (a: AttemptFinished) => void;
  onRunProgress?: (p: { runId: string; completed: number; total: number }) => void;
  onRunFinished?: (r: RunFinished) => void;
}): Promise<UnlistenFn[]> {
  const unlistens: UnlistenFn[] = [];
  if (handlers.onAttemptFinished) {
    unlistens.push(
      await listen<AttemptFinished>('test-attempt-finished', (e) => handlers.onAttemptFinished!(e.payload)),
    );
  }
  if (handlers.onRunProgress) {
    unlistens.push(
      await listen<{ runId: string; completed: number; total: number }>('test-run-progress', (e) =>
        handlers.onRunProgress!(e.payload),
      ),
    );
  }
  if (handlers.onRunFinished) {
    unlistens.push(await listen<RunFinished>('test-run-finished', (e) => handlers.onRunFinished!(e.payload)));
  }
  return unlistens;
}

/** 从目录与选中集合构造测试输入 */
export function buildInput(
  app: TabAppType,
  catalog: ProviderCatalogView[],
  selected: Set<string>,
  opts: {
    attemptsPerModel: number;
    timeoutSeconds: number;
    mode: TestMode;
    globalConcurrency: number;
    providerConcurrency: number;
    testAllCandidateEndpoints: boolean;
    applyBodyOverrides: boolean;
  },
  emuOverrides: Record<string, boolean> = {},
): TestRunInput | { error: string } {
  const providerIds = new Set<string>();
  const modelKeys: string[] = [];
  const emulationOverrides: Record<string, boolean> = {};
  for (const p of catalog) {
    for (const m of p.models) {
      const key = selectionKey(app, p.id, m.modelId);
      if (selected.has(key)) {
        providerIds.add(p.id);
        modelKeys.push(key);
        // 全量传递生效值：手动覆盖 > 供应商配置默认值（claude/codex 供应商默认关）
        emulationOverrides[key] = emuOverrides[key] ?? p.clientEmulation?.enabled ?? false;
      }
    }
  }
  if (modelKeys.length === 0) return { error: '没有选中任何模型' };
  const notTestable = catalog.some(
    (p) => providerIds.has(p.id) && p.status !== 'ready',
  );
  void notTestable;
  return {
    app,
    providerIds: [...providerIds],
    modelKeys,
    emulationOverrides,
    ...opts,
  };
}

export { errText };
