// Tauri 命令调用封装
import { invoke } from '@tauri-apps/api/core';
import type {
  AppInfo,
  AppSettingsView,
  HistoryAttempt,
  HistoryRun,
  PromptItem,
  ProviderCatalogView,
  RuleItem,
  SourceInfo,
  TabAppType,
} from './types';

export function getAppInfo(): Promise<AppInfo> {
  return invoke('get_app_info');
}

export function getCcSwitchSource(): Promise<SourceInfo> {
  return invoke('get_ccswitch_source');
}

export function loadProviderCatalog(appType: TabAppType): Promise<ProviderCatalogView[]> {
  return invoke('load_provider_catalog', { appType });
}

// ==================== M5：设置 ====================

export function getAppSettings(): Promise<AppSettingsView> {
  return invoke('get_app_settings');
}

export function setProxyUrl(url: string): Promise<void> {
  return invoke('set_proxy_url', { url });
}

export function setCcSwitchSource(path: string): Promise<SourceInfo> {
  return invoke('set_ccswitch_source', { path });
}

export function listPrompts(): Promise<PromptItem[]> {
  return invoke('list_prompts');
}

export function savePrompt(id: string | null, name: string, content: string, enabled: boolean): Promise<string> {
  return invoke('save_prompt', { id, name, content, enabled });
}

export function deletePrompt(id: string): Promise<void> {
  return invoke('delete_prompt', { id });
}

export function listRules(): Promise<RuleItem[]> {
  return invoke('list_rules');
}

export function saveRule(id: string | null, pattern: string, enabled: boolean): Promise<string> {
  return invoke('save_rule', { id, pattern, enabled });
}

export function deleteRule(id: string): Promise<void> {
  return invoke('delete_rule', { id });
}

// ==================== M5：测试历史 ====================

export function historyRuns(): Promise<HistoryRun[]> {
  return invoke('history_runs');
}

export function historyAttempts(runId: string): Promise<HistoryAttempt[]> {
  return invoke('history_attempts', { runId });
}

export function historyDeleteRun(runId: string): Promise<void> {
  return invoke('history_delete_run', { runId });
}

export function historyClear(): Promise<void> {
  return invoke('history_clear');
}
