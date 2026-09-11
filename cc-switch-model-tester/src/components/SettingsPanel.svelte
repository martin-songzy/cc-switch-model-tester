<script lang="ts">
  import { onMount } from 'svelte';
  import {
    deletePrompt,
    deleteRule,
    getAppSettings,
    listPrompts,
    listRules,
    savePrompt,
    saveRule,
    setCcSwitchSource,
    setProxyUrl,
  } from '../lib/api';
  import type { AppSettingsView, PromptItem, RuleItem, SourceInfo } from '../lib/types';

  let {
    source,
    onSourceChanged,
  }: {
    source: SourceInfo | null;
    onSourceChanged: (info: SourceInfo) => void;
  } = $props();

  let settings = $state<AppSettingsView | null>(null);
  let proxyInput = $state('');
  let ccPathInput = $state('');
  let prompts = $state<PromptItem[]>([]);
  let rules = $state<RuleItem[]>([]);
  let flash = $state('');
  let errorMsg = $state('');
  let newPromptName = $state('');
  let newPromptContent = $state('');
  let newRulePattern = $state('');

  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  function showFlash(msg: string) {
    flash = msg;
    errorMsg = '';
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (flash = ''), 2500);
  }
  function showError(e: unknown) {
    errorMsg = typeof e === 'string' ? e : String((e as { message?: string })?.message ?? e);
  }

  onMount(async () => {
    await reloadAll();
  });

  async function reloadAll() {
    try {
      const [s, p, r] = await Promise.all([getAppSettings(), listPrompts(), listRules()]);
      settings = s;
      proxyInput = s.proxyUrl ?? '';
      ccPathInput = s.ccswitchDbPath ?? '';
      prompts = p;
      rules = r;
    } catch (e) {
      showError(e);
    }
  }

  async function saveProxy() {
    try {
      await setProxyUrl(proxyInput.trim());
      showFlash(proxyInput.trim() ? '代理已保存' : '已恢复默认代理');
      await reloadAll();
    } catch (e) {
      showError(e);
    }
  }

  function useDefaultProxy() {
    proxyInput = '';
    void saveProxy();
  }

  async function saveSource() {
    try {
      const info = await setCcSwitchSource(ccPathInput.trim());
      showFlash('数据源已保存');
      onSourceChanged(info);
      await reloadAll();
    } catch (e) {
      showError(e);
    }
  }

  async function resetSource() {
    try {
      const info = await setCcSwitchSource('');
      ccPathInput = '';
      showFlash('已恢复默认数据源路径');
      onSourceChanged(info);
      await reloadAll();
    } catch (e) {
      showError(e);
    }
  }

  async function saveOnePrompt(p: PromptItem) {
    try {
      await savePrompt(p.id, p.name, p.content, p.enabled);
      showFlash('提示词已保存');
      await reloadAll();
    } catch (e) {
      showError(e);
      await reloadAll(); // 还原界面上被拒绝的修改
    }
  }

  async function removeOnePrompt(p: PromptItem) {
    try {
      await deletePrompt(p.id);
      await reloadAll();
    } catch (e) {
      showError(e);
    }
  }

  async function addPrompt() {
    if (!newPromptContent.trim()) return;
    try {
      await savePrompt(null, newPromptName.trim() || '自定义', newPromptContent, true);
      newPromptName = '';
      newPromptContent = '';
      showFlash('已新增提示词');
      await reloadAll();
    } catch (e) {
      showError(e);
    }
  }

  async function saveOneRule(r: RuleItem) {
    try {
      await saveRule(r.id, r.pattern, r.enabled);
      showFlash('规则已保存');
      await reloadAll();
    } catch (e) {
      showError(e);
      await reloadAll();
    }
  }

  async function removeOneRule(r: RuleItem) {
    try {
      await deleteRule(r.id);
      await reloadAll();
    } catch (e) {
      showError(e);
    }
  }

  async function addRule() {
    if (!newRulePattern.trim()) return;
    try {
      await saveRule(null, newRulePattern, true);
      newRulePattern = '';
      showFlash('已新增负面规则');
      await reloadAll();
    } catch (e) {
      showError(e);
    }
  }

  function fmtPath(s: SourceInfo | null): string {
    if (!s || !s.path) return '未知';
    return s.path;
  }
</script>

<div class="settings-wrap">
  <div class="card">
    <h3>cc-switch 数据源</h3>
    <p class="hint">
      cc-switch 数据库（cc-switch.db）的完整路径。默认自动使用
      <code>{settings?.defaultCcswitchPath || '~/.cc-switch/cc-switch.db'}</code>
      ；以后 cc-switch 换了安装位置，在这里填新路径即可，无需改代码。
    </p>
    <div class="row">
      <input
        class="text-input grow"
        type="text"
        bind:value={ccPathInput}
        placeholder="留空使用默认路径"
        spellcheck="false"
      />
      <button class="btn" onclick={saveSource}>保存</button>
      <button class="btn ghost" onclick={resetSource}>恢复默认</button>
    </div>
    <p class="hint status">
      当前数据源：<code>{fmtPath(source)}</code>
      {#if source}
        {#if source.error}
          <span class="bad">（异常：{source.error.message}）</span>
        {:else}
          <span class="good">（存在，schema {source.schemaVersion ?? '?'}）</span>
        {/if}
      {/if}
    </p>
  </div>

  <div class="card">
    <h3>代理</h3>
    <p class="hint">
      测试请求经此代理发出，默认 <code>{settings?.defaultProxy || 'socks5://127.0.0.1:1080'}</code>；
      填 <code>direct</code> 表示强制直连。修改后对下一轮测试生效。
    </p>
    <div class="row">
      <input
        class="text-input grow"
        type="text"
        bind:value={proxyInput}
        placeholder="socks5://127.0.0.1:1080"
        spellcheck="false"
      />
      <button class="btn" onclick={saveProxy}>保存</button>
      <button class="btn ghost" onclick={useDefaultProxy}>恢复默认</button>
    </div>
  </div>

  <div class="card">
    <h3>测试提示词</h3>
    <p class="hint">
      每次测试随机使用「已启用」提示词中的一条；内置 8 条可改文字、可停用，但不能删除。
    </p>
    <div class="list-head">
      <span class="col-enable">启用</span>
      <span class="col-name">名称</span>
      <span class="col-content">内容</span>
      <span class="col-ops"></span>
    </div>
    {#each prompts as p (p.id)}
      <div class="list-row">
        <span class="col-enable">
          <input type="checkbox" bind:checked={p.enabled} onchange={() => saveOnePrompt(p)} />
        </span>
        <input class="text-input col-name" type="text" bind:value={p.name} />
        <input class="text-input col-content" type="text" bind:value={p.content} spellcheck="false" />
        <span class="col-ops">
          <button class="btn sm" onclick={() => saveOnePrompt(p)}>保存</button>
          <button class="btn sm ghost" disabled={p.isBuiltin} onclick={() => removeOnePrompt(p)}>
            删除
          </button>
        </span>
      </div>
    {/each}
    <div class="list-row add-row">
      <span class="col-enable">新增</span>
      <input class="text-input col-name" type="text" bind:value={newPromptName} placeholder="名称（可留空）" />
      <input class="text-input col-content" type="text" bind:value={newPromptContent} placeholder="提示词内容，例如：直接回复pong" spellcheck="false" />
      <span class="col-ops">
        <button class="btn sm" onclick={addPrompt}>添加</button>
      </span>
    </div>
  </div>

  <div class="card">
    <h3>负面规则</h3>
    <p class="hint">
      响应中包含「已启用」规则里的文字时，判定为失败（不区分大小写）。内置 8 条可改、可停用、不可删除。
    </p>
    <div class="list-head two-col">
      <span class="col-enable">启用</span>
      <span class="col-content wide">规则文字</span>
      <span class="col-ops"></span>
    </div>
    {#each rules as r (r.id)}
      <div class="list-row two-col">
        <span class="col-enable">
          <input type="checkbox" bind:checked={r.enabled} onchange={() => saveOneRule(r)} />
        </span>
        <input class="text-input col-content wide" type="text" bind:value={r.pattern} spellcheck="false" />
        <span class="col-ops">
          <button class="btn sm" onclick={() => saveOneRule(r)}>保存</button>
          <button class="btn sm ghost" disabled={r.isBuiltin} onclick={() => removeOneRule(r)}>
            删除
          </button>
        </span>
      </div>
    {/each}
    <div class="list-row two-col add-row">
      <span class="col-enable">新增</span>
      <input class="text-input col-content wide" type="text" bind:value={newRulePattern} placeholder="规则文字，例如：rate limit" spellcheck="false" />
      <span class="col-ops">
        <button class="btn sm" onclick={addRule}>添加</button>
      </span>
    </div>
  </div>

  {#if flash}<div class="toast good">{flash}</div>{/if}
  {#if errorMsg}<div class="toast bad">{errorMsg}</div>{/if}
</div>

<style>
  .settings-wrap {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 20px 24px;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
    box-sizing: border-box;
  }
  .card {
    background: #fff;
    border: 1px solid #e3e6ea;
    border-radius: 10px;
    padding: 14px 18px 16px;
  }
  h3 {
    margin: 0 0 6px;
    font-size: 14px;
    color: #3d4656;
  }
  .hint {
    margin: 0 0 10px;
    font-size: 12px;
    color: #8a93a5;
    line-height: 1.6;
  }
  code {
    background: #f2f4f8;
    border-radius: 4px;
    padding: 0 4px;
    font-size: 11px;
    color: #5b6472;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .text-input {
    border: 1px solid #dde1e8;
    border-radius: 6px;
    padding: 5px 8px;
    font-size: 12px;
    color: #3d4656;
    min-width: 0;
  }
  .text-input:focus {
    outline: none;
    border-color: #3b6ef6;
  }
  .grow {
    flex: 1;
  }
  .status {
    margin-top: 8px;
    margin-bottom: 0;
  }
  .good {
    color: #1f9d61;
  }
  .bad {
    color: #d64545;
  }

  /* 提示词 / 规则列表 */
  .list-head,
  .list-row {
    display: grid;
    grid-template-columns: 40px minmax(120px, 160px) 1fr 132px;
    gap: 8px;
    align-items: center;
    padding: 4px 0;
  }
  .list-head {
    font-size: 11px;
    color: #8a93a5;
    border-bottom: 1px solid #eef0f4;
    padding-bottom: 6px;
  }
  .list-row.add-row {
    border-top: 1px dashed #e3e6ea;
    margin-top: 4px;
    padding-top: 8px;
  }
  /* 负面规则列表只有两列内容 */
  .list-head.two-col,
  .list-row.two-col {
    grid-template-columns: 40px 1fr 132px;
  }
  .col-enable {
    text-align: center;
  }
  .col-ops {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }

  .btn {
    border: 1px solid #dde1e8;
    background: #fff;
    color: #3d4656;
    border-radius: 6px;
    padding: 5px 12px;
    font-size: 12px;
    cursor: pointer;
  }
  .btn:hover {
    border-color: #3b6ef6;
    color: #3b6ef6;
  }
  .btn.sm {
    padding: 3px 9px;
    font-size: 11px;
  }
  .btn.ghost {
    background: transparent;
  }
  .btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .toast {
    position: fixed;
    top: 60px;
    left: 50%;
    transform: translateX(-50%);
    background: #fff;
    border: 1px solid #e3e6ea;
    border-radius: 8px;
    padding: 8px 18px;
    font-size: 13px;
    box-shadow: 0 4px 16px rgba(20, 30, 60, 0.12);
    z-index: 60;
  }
</style>
