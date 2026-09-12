<script lang="ts">
  import { onMount } from 'svelte';
  import HistoryPanel from '../components/HistoryPanel.svelte';
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import TestPanel from '../components/TestPanel.svelte';
  import { getAppInfo, getCcSwitchSource, loadProviderCatalog } from '../lib/api';
  import {
    TABS,
    errText,
    selectionKey,
    type AppInfo,
    type ModelView,
    type ProviderCatalogView,
    type SourceInfo,
    type TabAppType,
  } from '../lib/types';

  let appInfo = $state<AppInfo | null>(null);
  let source = $state<SourceInfo | null>(null);
  let catalogs = $state<Record<TabAppType, ProviderCatalogView[]>>({
    claude: [],
    codex: [],
    pi: [],
  });
  let loadError = $state<string | null>(null);
  let loading = $state(false);
  let activeTab = $state<TabAppType>('claude');
  let search = $state('');
  let statusFilter = $state<'all' | 'ready'>('all');
  let expanded = $state<Record<string, boolean>>({});
  /** 全局选中集合：key = providerId::modelId */
  let selected = $state<Set<string>>(new Set());

  // ---- 客户端仿真：每模型画像选择（localStorage 持久化；默认跟随供应商配置）----
  const EMU_LS_KEY = 'tester.emulation.v2';
  let emuOverrides = $state<Record<string, string>>(readEmuStore());

  function readEmuStore(): Record<string, string> {
    try {
      return JSON.parse(localStorage.getItem(EMU_LS_KEY) ?? '{}');
    } catch {
      return {};
    }
  }
  function persistEmu() {
    try {
      localStorage.setItem(EMU_LS_KEY, JSON.stringify(emuOverrides));
    } catch { /* 忽略存储失败 */ }
  }

  /** 可选画像（与后端 PROFILES 一致） */
  const EMU_PROFILES = [
    { value: '', label: '不仿真' },
    { value: 'claude-code', label: 'Claude Code' },
    { value: 'codex', label: 'Codex' },
    { value: 'gemini-cli', label: 'Gemini CLI' },
  ];
  /** 模型的仿真生效值（画像名；'' = 关）：手动选择 > 供应商配置默认值 */
  function emulationOf(p: ProviderCatalogView, m: ModelView): string {
    const k = selectionKey(activeTab, p.id, m.modelId);
    return emuOverrides[k] ?? (p.clientEmulation?.enabled ? p.clientEmulation.profile : '');
  }
  function setEmulation(p: ProviderCatalogView, m: ModelView, profile: string) {
    const k = selectionKey(activeTab, p.id, m.modelId);
    emuOverrides[k] = profile;
    persistEmu();
    openEmuMenu = null;
  }
  /** 正在展开选择菜单的模型 key */
  let openEmuMenu = $state<string | null>(null);

  // ---- 布局：目录区高度可拖动；测试面板可开关 ----
  let panelOpen = $state(false);
  let catalogOpen = $state(true);
  /** 主视图切换：目录（默认）/ 历史 / 设置 */
  let activeView = $state<'catalog' | 'history' | 'settings'>('catalog');

  /** 设置页保存数据源后：更新状态并重载目录 */
  async function onSourceChanged(info: SourceInfo) {
    source = info;
    loadError = null;
    if (!info.error && info.exists) await refreshAll();
  }
  let dirHeight = $state(420);
  let dragging = $state(false);
  let dragStartY = $state(0);
  let dragStartH = $state(0);

  function startDirResize(e: MouseEvent) {
    dragging = true;
    dragStartY = e.clientY;
    dragStartH = dirHeight;
  }

  function onWindowMousemove(e: MouseEvent) {
    if (!dragging) return;
    const max = window.innerHeight - 260;
    dirHeight = Math.min(Math.max(dragStartH + (e.clientY - dragStartY), 140), max);
  }

  function onWindowMouseup() {
    dragging = false;
  }

  /** 搜索关键词：空格分隔，AND 逻辑（同时满足） */
  const keywords = $derived(
    search
      .trim()
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean),
  );
  const searchActive = $derived(keywords.length > 0);

  const PROTOCOL_LABELS: Record<string, string> = {
    anthropic_messages: 'Anthropic Messages',
    openai_chat: 'OpenAI Chat',
    openai_responses: 'OpenAI Responses',
    gemini_native: 'Gemini Native',
    bedrock_converse_stream: 'Bedrock',
  };

  function protocolLabel(p: string): string {
    return PROTOCOL_LABELS[p] ?? p;
  }

  function statusClass(s: string): string {
    switch (s) {
      case 'ready': return 'ok';
      case 'config_error': return 'err';
      case 'unsupported_protocol': return 'orange';
      default: return 'gray';
    }
  }

  // ==================== 站点禁用测试（点击“可测试”徽章切换） ====================
  const DISABLED_KEY = 'tester.disabled.v1';
  /** key：`{app}::{providerId}`；禁用后排除出后续测试，可再次点击恢复 */
  let disabledProviders = $state<Set<string>>(new Set());

  function disabledKey(p: ProviderCatalogView): string {
    return `${activeTab}::${p.id}`;
  }

  function isProviderDisabled(p: ProviderCatalogView): boolean {
    return disabledProviders.has(disabledKey(p));
  }

  function toggleProviderDisabled(p: ProviderCatalogView) {
    if (!isTestableBase(p)) return;
    const k = disabledKey(p);
    const next = new Set(disabledProviders);
    if (next.has(k)) {
      next.delete(k);
    } else {
      next.add(k);
      // 禁用时自动清除该供应商的已选模型，避免残留到测试输入
      const nextSelected = new Set(selected);
      for (const m of p.models) nextSelected.delete(selectionKey(activeTab, p.id, m.modelId));
      selected = nextSelected;
    }
    disabledProviders = next;
  }

  try {
    const raw = localStorage.getItem(DISABLED_KEY);
    if (raw) {
      const arr = JSON.parse(raw) as unknown;
      if (Array.isArray(arr)) disabledProviders = new Set(arr.filter((x): x is string => typeof x === 'string'));
    }
  } catch {
    // 损坏数据按空处理
  }

  $effect(() => {
    try {
      localStorage.setItem(DISABLED_KEY, JSON.stringify([...disabledProviders]));
    } catch {
      // 存储不可用时忽略
    }
  });

  /** 基础可测性：后端状态 + 有模型（不含手动禁用） */
  function isTestableBase(p: ProviderCatalogView): boolean {
    return p.status === 'ready' && p.models.length > 0;
  }

  function isTestable(p: ProviderCatalogView): boolean {
    return isTestableBase(p) && !isProviderDisabled(p);
  }

  /** 供应商自身（名称/ID）是否命中所有关键词 */
  function providerMatches(p: ProviderCatalogView): boolean {
    if (keywords.length === 0) return true;
    const hay = `${p.name} ${p.id}`.toLowerCase();
    return keywords.every((k) => hay.includes(k));
  }

  /** 单个模型是否命中所有关键词 */
  function modelMatches(p: ProviderCatalogView, m: { modelId: string; displayName: string | null }): boolean {
    if (keywords.length === 0) return true;
    const hay = `${m.modelId} ${m.displayName ?? ''}`.toLowerCase();
    return keywords.every((k) => hay.includes(k));
  }

  /** 搜索状态下该供应商应显示的模型：供应商自身命中 → 全部；否则仅显示命中的模型 */
  function visibleModels(p: ProviderCatalogView) {
    if (keywords.length === 0) return p.models;
    if (providerMatches(p)) return p.models;
    return p.models.filter((m) => modelMatches(p, m));
  }

  /** 当前标签页经筛选后的供应商（自身命中或有模型命中） */
  const filtered = $derived(
    catalogs[activeTab].filter((p) => {
      if (statusFilter === 'ready' && p.status !== 'ready') return false;
      if (keywords.length === 0) return true;
      return providerMatches(p) || p.models.some((m) => modelMatches(p, m));
    }),
  );

  const currentList = $derived(catalogs[activeTab]);

  function providerSelectedCount(p: ProviderCatalogView): number {
    let n = 0;
    for (const m of p.models) if (selected.has(selectionKey(activeTab, p.id, m.modelId))) n++;
    return n;
  }

  /** 供应商复选框状态基于可见模型（搜索过滤时与界面一致） */
  function providerCheckState(p: ProviderCatalogView): 'all' | 'some' | 'none' {
    const visible = visibleModels(p);
    if (!isTestable(p) || visible.length === 0) return 'none';
    const n = visible.filter((m) => selected.has(selectionKey(activeTab, p.id, m.modelId))).length;
    if (n === 0) return 'none';
    return n === visible.length ? 'all' : 'some';
  }

  function toggleProvider(p: ProviderCatalogView) {
    const visible = visibleModels(p);
    if (visible.length === 0) return;
    const state = providerCheckState(p);
    const next = new Set(selected);
    const wantSelect = state !== 'all';
    for (const m of visible) {
      const k = selectionKey(activeTab, p.id, m.modelId);
      if (wantSelect) next.add(k);
      else next.delete(k);
    }
    selected = next;
  }

  function toggleModel(p: ProviderCatalogView, modelId: string) {
    const k = selectionKey(activeTab, p.id, modelId);
    const next = new Set(selected);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    selected = next;
  }

  /** “当前结果”＝每个供应商当前可见的模型（与界面显示一致） */
  function selectAllFiltered() {
    const next = new Set(selected);
    for (const p of filtered) {
      if (!isTestable(p)) continue;
      for (const m of visibleModels(p)) next.add(selectionKey(activeTab, p.id, m.modelId));
    }
    selected = next;
  }

  function invertFiltered() {
    const next = new Set(selected);
    for (const p of filtered) {
      if (!isTestable(p)) continue;
      for (const m of visibleModels(p)) {
        const k = selectionKey(activeTab, p.id, m.modelId);
        if (next.has(k)) next.delete(k);
        else next.add(k);
      }
    }
    selected = next;
  }

  function clearSelection() {
    selected = new Set();
  }

  /** 搜索从“有条件”变为“清空”时：自动展开所有含有已选模型的供应商，便于检查选中情况 */
  let prevSearchActive = $state(false);
  $effect(() => {
    const active = searchActive;
    if (prevSearchActive && !active) {
      const exp = { ...expanded };
      let changed = false;
      for (const tab of TABS) {
        for (const p of catalogs[tab.key]) {
          if (providerSelectedCount(p) > 0 && !exp[p.id]) {
            exp[p.id] = true;
            changed = true;
          }
        }
      }
      if (changed) expanded = exp;
    }
    prevSearchActive = active;
  });

  const totalSelected = $derived(selected.size);

  /** 当前标签页已选目标数 */
  const tabSelectedCount = $derived(
    catalogs[activeTab].reduce((acc, p) => acc + providerSelectedCount(p), 0),
  );

  async function refreshAll() {
    loading = true;
    loadError = null;
    try {
      source = await getCcSwitchSource();
      if (!source.error && source.exists) {
        const [claude, codex, pi] = await Promise.all([
          loadProviderCatalog('claude'),
          loadProviderCatalog('codex'),
          loadProviderCatalog('pi'),
        ]);
        catalogs = { claude, codex, pi };
        // 默认展开有内容的供应商（首个）
        const exp: Record<string, boolean> = {};
        for (const tab of TABS) {
          catalogs[tab.key].forEach((p, i) => {
            exp[p.id] = i === 0;
          });
        }
        expanded = exp;
      }
    } catch (e) {
      loadError = errText(e);
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    try {
      appInfo = await getAppInfo();
    } catch (e) {
      loadError = errText(e);
    }
    await refreshAll();
  });
</script>

<svelte:window onmousemove={onWindowMousemove} onmouseup={onWindowMouseup} />

<main>
  <header>
    <div class="brand">
      <h1>cc-switch Model Tester</h1>
      {#if appInfo}<span class="version">v{appInfo.version} ({appInfo.commit})</span>{/if}
    </div>

    <div class="source" class:bad={!!source?.error}>
      {#if !source}
        <span class="muted">正在读取数据源…</span>
      {:else if source.error}
        <span class="badge err">数据源异常</span>
        <span class="muted">{source.error.message}</span>
      {:else}
        <span class="badge ok">已连接</span>
        <span class="mono path" title={source.path}>{source.path}</span>
        <span class="badge">schema {source.schemaVersion}</span>
      {/if}
    </div>

    <button class="btn" onclick={refreshAll} disabled={loading}>
      {loading ? '加载中…' : '重新加载'}
    </button>
  </header>

  {#if loadError}
    <div class="banner err-banner">加载失败：{loadError}</div>
  {/if}

  <nav>
    {#each TABS as tab (tab.key)}
      <button
        class="tab"
        class:active={activeTab === tab.key}
        onclick={() => (activeTab = tab.key)}
      >
        {tab.label}
        <span class="tab-count">{catalogs[tab.key].length}</span>
      </button>
    {/each}
    <span class="nav-spacer"></span>
    <button
      class="tab"
      class:active={activeView === 'catalog'}
      onclick={() => (activeView = 'catalog')}
    >测试</button>
    <button
      class="tab"
      class:active={activeView === 'history'}
      onclick={() => (activeView = 'history')}
    >历史</button>
    <button
      class="tab"
      class:active={activeView === 'settings'}
      onclick={() => (activeView = 'settings')}
    >设置</button>
  </nav>

  <div class="toolbar" style={activeView === 'catalog' ? '' : 'display:none;'}>
    <input
      class="search"
      type="search"
      placeholder="搜索：支持多关键词，空格分隔需同时满足（如 gpt 6）…"
      bind:value={search}
    />
    <label class="filter">
      <input
        type="checkbox"
        checked={statusFilter === 'ready'}
        onchange={(e) => (statusFilter = e.currentTarget.checked ? 'ready' : 'all')}
      />
      只看可测试
    </label>
    <button
      class="btn sm"
      onclick={() => {
        const anyClosed = filtered.some((p) => isTestableBase(p) && !expanded[p.id]);
        const exp: Record<string, boolean> = { ...expanded };
        for (const p of filtered) {
          if (isTestableBase(p)) exp[p.id] = anyClosed;
        }
        expanded = exp;
      }}
      title="在当前筛选结果中切换全部展开/全部折叠"
    >
      {filtered.some((p) => isTestableBase(p) && !expanded[p.id]) ? '全部展开' : '全部折叠'}
    </button>
    <div class="spacer"></div>
    <button class="btn sm" onclick={selectAllFiltered}>全选当前结果</button>
    <button class="btn sm" onclick={invertFiltered}>反选</button>
    <button class="btn sm" onclick={clearSelection}>清空</button>
    <button class="btn sm" class:active={catalogOpen} onclick={() => (catalogOpen = !catalogOpen)}>
      {catalogOpen ? '收起目录' : '打开目录'}
    </button>
    <button class="btn sm" class:active={panelOpen} onclick={() => (panelOpen = !panelOpen)}>
      {panelOpen ? '收起测试面板' : '打开测试面板'}
    </button>
  </div>

  <section
    class="content"
    style={activeView !== 'catalog' || !catalogOpen
      ? 'display:none;'
      : panelOpen
        ? `height:${dirHeight}px; flex:none;`
        : 'flex:1;'}
  >
    {#if source?.error || loadError}
      <p class="empty">数据源不可用，无法加载供应商目录。</p>
    {:else if currentList.length === 0}
      <p class="empty">该类型下暂无供应商（cc-switch 中未配置）。</p>
    {:else if filtered.length === 0}
      <p class="empty">无匹配结果。</p>
    {:else}
      {#each filtered as p (p.id)}
        <div class="provider" class:disabled={!isTestable(p)}>
          <div class="prov-row">
            {#if isTestable(p)}
              <input
                type="checkbox"
                class="prov-check"
                checked={providerCheckState(p) === 'all'}
                indeterminate={providerCheckState(p) === 'some'}
                onchange={() => toggleProvider(p)}
                title="全选 / 取消该供应商的所有模型"
              />
            {:else}
              <span class="check-placeholder"></span>
            {/if}

            <button
              class="expand"
              onclick={() => (expanded[p.id] = !expanded[p.id])}
              title={expanded[p.id] ? '收起模型列表' : '展开模型列表'}
            >
              {expanded[p.id] ? '▾' : '▸'}
            </button>

            <span
              class="prov-name"
              role="button"
              tabindex="0"
              onkeydown={(e) => {
                if (e.key === 'Enter') expanded[p.id] = !expanded[p.id];
              }}
              onclick={() => (expanded[p.id] = !expanded[p.id])}
              title="点击展开/收起模型列表"
            >{p.name}</span>
            <span
              class="mono muted prov-id"
              role="button"
              tabindex="0"
              onkeydown={(e) => {
                if (e.key === 'Enter') expanded[p.id] = !expanded[p.id];
              }}
              onclick={() => (expanded[p.id] = !expanded[p.id])}
              title="点击展开/收起模型列表"
            >{p.id}</span>

            {#if isTestableBase(p)}
              <button
                type="button"
                class="badge badge-toggle {isProviderDisabled(p) ? 'gray' : 'ok'}"
                onclick={() => toggleProviderDisabled(p)}
                title={isProviderDisabled(p) ? '点击重新启用测试' : '点击禁用测试（排除出后续测试，模型勾选将清空）'}
              >
                {isProviderDisabled(p) ? '禁用测试' : p.statusLabel}
              </button>
            {:else}
              <span class="badge {statusClass(p.status)}">{p.statusLabel}</span>
            {/if}
            {#if isTestable(p)}
              <span class="badge">{protocolLabel(p.protocol)}</span>
              <span class="badge">{p.credentialLabel}</span>
              {#if p.candidateEndpointCount > 0}
                <span class="badge" title="另有 {p.candidateEndpointCount} 个候选端点（默认仅测主端点）">+{p.candidateEndpointCount} 候选</span>
              {/if}
            {/if}

            <span class="spacer"></span>
            <span class="muted models-count">{
              searchActive && !providerMatches(p)
                ? `${visibleModels(p).length}/${p.models.length} 模型`
                : `${p.models.length} 模型`
            }{providerSelectedCount(p) > 0 ? ` · 已选 ${providerSelectedCount(p)}` : ''}</span>
          </div>

          {#if p.error}
            <div class="prov-detail err-text">错误：{p.error}</div>
          {/if}
          {#each p.warnings as w}
            <div class="prov-detail warn-text" title={w}>⚠ {w}</div>
          {/each}

          {#if (searchActive || expanded[p.id]) && visibleModels(p).length > 0}
            <div class="models">
              {#each visibleModels(p) as m (m.modelId)}
                <label class="model-row" class:off={!isTestable(p)}>
                  {#if isTestable(p)}
                    <input
                      type="checkbox"
                      checked={selected.has(selectionKey(activeTab, p.id, m.modelId))}
                      onchange={() => toggleModel(p, m.modelId)}
                    />
                  {:else}
                    <span class="check-placeholder"></span>
                  {/if}
                  <span class="mono model-id">{m.modelId}</span>
                  {#if m.displayName && m.displayName !== m.modelId}
                    <span class="muted model-display">{m.displayName}</span>
                  {/if}
                  <span class="spacer model-spacer"></span>
                  <button
                    type="button"
                    class="emu-toggle"
                    class:on={emulationOf(p, m) !== ''}
                    onclick={(e) => {
                      e.preventDefault();
                      openEmuMenu = openEmuMenu === selectionKey(activeTab, p.id, m.modelId) ? null : selectionKey(activeTab, p.id, m.modelId);
                    }}
                    title="客户端仿真：选择模拟的官方客户端指纹（可跨协议选择）"
                  >{emulationOf(p, m) === '' ? '🎭 仿真' : `🎭 ${emulationOf(p, m)}`}</button>
                  {#if openEmuMenu === selectionKey(activeTab, p.id, m.modelId)}
                    <div class="emu-menu" role="menu">
                      {#each EMU_PROFILES as opt (opt.value)}
                        <button
                          type="button"
                          class:current={emulationOf(p, m) === opt.value}
                          onclick={(e) => {
                            e.preventDefault();
                            setEmulation(p, m, opt.value);
                          }}
                        >{emulationOf(p, m) === opt.value ? '✓ ' : ''}{opt.label}</button>
                      {/each}
                    </div>
                  {/if}
                </label>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </section>

  {#if activeView === 'catalog' && panelOpen && catalogOpen}
    <button type="button"
      class="splitter"
      aria-label="拖动调整目录区高度"
      onmousedown={startDirResize}
      title="拖动调整目录区高度"
    ></button>
  {/if}
  <div
    class="test-area"
    style={activeView === 'catalog' && panelOpen ? 'flex:1;' : 'display:none;'}
  >
      {#if !(source?.error || loadError)}
        <TestPanel
        app={activeTab}
        catalog={() => catalogs[activeTab]}
        selected={() => selected}
        emuOverrides={emuOverrides}
      />
      {:else}
        <p class="empty">数据源不可用</p>
      {/if}
  </div>

  {#if activeView === 'history'}
    <HistoryPanel />
  {:else if activeView === 'settings'}
    <SettingsPanel {source} onSourceChanged={onSourceChanged} />
  {/if}

  <footer>
    <span>
      当前标签页已选 <b>{tabSelectedCount}</b> 个测试目标
      {#if totalSelected !== tabSelectedCount}
        （全部标签页共 {totalSelected}）
      {/if}
    </span>
  </footer>
</main>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    height: 100%;
    font-family: 'Segoe UI', 'Microsoft YaHei', system-ui, sans-serif;
    font-size: 14px;
    color: #1f2430;
    background: #f5f6f8;
  }
  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 12px 16px;
    box-sizing: border-box;
    gap: 10px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .brand { display: flex; align-items: baseline; gap: 8px; }
  h1 { font-size: 17px; margin: 0; white-space: nowrap; }
  .version { color: #8a93a5; font-size: 12px; }
  .source { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; flex-wrap: wrap; }
  .path { max-width: 380px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge {
    background: #e8ebf0; border-radius: 10px; padding: 2px 10px;
    font-size: 12px; white-space: nowrap;
  }
  .badge.ok { background: #d9f2e0; color: #1a7a3c; }
  .badge.err { background: #fbe0e0; color: #b03030; }
  .badge.warn { background: #fdf0d9; color: #9a6b0f; }
  .badge.gray { background: #e8ebf0; color: #6b7385; }
  .badge.orange { background: #fde8d3; color: #a05a12; }
  .mono { font-family: Consolas, 'Cascadia Mono', monospace; font-size: 12px; }
  .muted { color: #8a93a5; }
  .btn {
    border: 1px solid #c9cfda; background: #fff; border-radius: 6px;
    padding: 5px 14px; cursor: pointer; font-size: 13px;
  }
  .btn:hover:not(:disabled) { background: #f0f2f6; }
  .btn:disabled { color: #aab1bf; cursor: default; }
  .btn.sm { padding: 3px 10px; font-size: 12px; }
  .banner { border-radius: 6px; padding: 8px 12px; font-size: 13px; }
  .err-banner { background: #fbe0e0; color: #b03030; }
  nav { display: flex; gap: 4px; border-bottom: 1px solid #dde1e8; }
  .nav-spacer { flex: 1; }
  .tab {
    border: none; background: transparent; padding: 8px 16px; font-size: 14px;
    cursor: pointer; color: #5a6375; border-bottom: 2px solid transparent; margin-bottom: -1px;
  }
  .tab.active { color: #1f2430; font-weight: 600; border-bottom-color: #3b6ef6; }
  .tab-count {
    background: #e8ebf0; border-radius: 9px; padding: 1px 7px; font-size: 11px;
    margin-left: 4px; color: #5a6375;
  }
  .toolbar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .search {
    flex: 0 1 320px; border: 1px solid #c9cfda; border-radius: 6px;
    padding: 6px 10px; font-size: 13px; background: #fff;
  }
  .filter { display: flex; align-items: center; gap: 5px; font-size: 13px; color: #5a6375; }
  .spacer { flex: 1; }
  .content {
    flex: 1; overflow: auto; background: #fff;
    border: 1px solid #e3e6ec; border-radius: 8px; padding: 6px 0;
  }
  .splitter {cursor: row-resize; border: none; background: #e3e6ec; padding: 0;
    height: 7px; cursor: row-resize; border-radius: 3px;
    background: #dfe3ea; flex: none;
  }
  .splitter:hover { background: #3b6ef6; }
  .test-area {
    flex: 1; min-height: 120px; overflow: auto; background: #fff;
    border: 1px solid #e3e6ec; border-radius: 8px; padding: 10px 12px;
  }
  .btn.active { background: #e4edfb; border-color: #3b6ef6; color: #2a5aa8; }
  .provider { border-bottom: 1px solid #f0f2f5; padding: 6px 14px; }
  .provider:last-child { border-bottom: none; }
  .provider.disabled { opacity: 0.62; }
  .prov-row { display: flex; align-items: center; gap: 8px; min-height: 28px; }
  .prov-check { width: 15px; height: 15px; cursor: pointer; }
  .check-placeholder { width: 15px; flex: none; }
  .expand {
    border: none; background: transparent; cursor: pointer; color: #5a6375;
    width: 22px; padding: 0; font-size: 12px;
  }
  .prov-name { font-weight: 600; cursor: pointer; }
  .prov-name:hover { color: #3b6ef6; }
  .prov-id { cursor: pointer; }
  .prov-id:hover { color: #3b6ef6; }
  .badge-toggle { cursor: pointer; border: none; }
  .badge-toggle:hover { filter: brightness(0.94); }
  .badge-toggle.gray { background: #e8ebf0; color: #6b7385; }
  .prov-id { max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .models-count { font-size: 12px; white-space: nowrap; }
  .prov-detail {
    font-size: 12px; margin: 2px 0 2px 45px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .err-text { color: #b03030; }
  .warn-text { color: #9a6b0f; }
  .models { margin: 4px 0 4px 45px; display: flex; flex-direction: column; gap: 2px; }
  .model-row {
    display: flex; align-items: center; gap: 8px; padding: 3px 8px;
    border-radius: 5px; cursor: pointer; font-size: 13px;
  }
  .model-row:hover { background: #f4f6fa; }
  .model-row.off { cursor: default; opacity: 0.6; }
  .model-row input { width: 14px; height: 14px; cursor: pointer; }
  .model-spacer { flex: 1; }
  .emu-toggle {
    border: 1px solid #dde1e8; background: #fff; border-radius: 10px;
    padding: 1px 8px; font-size: 11px; color: #8a93a5; cursor: pointer;
    white-space: nowrap; flex: none;
  }
  .emu-toggle:hover:not(:disabled) { border-color: #3b6ef6; color: #3b6ef6; }
  .emu-toggle.on {
    background: #e4edfb; border-color: #3b6ef6; color: #2a5aa8; font-weight: 600;
  }
  .emu-toggle:disabled { opacity: 0.4; cursor: not-allowed; }
  .model-row { position: relative; }
  .emu-menu {
    position: absolute; right: 4px; top: 100%; z-index: 30;
    background: #fff; border: 1px solid #dde1e8; border-radius: 6px;
    box-shadow: 0 4px 14px rgba(20, 30, 60, 0.14);
    display: flex; flex-direction: column; min-width: 130px; padding: 3px;
  }
  .emu-menu button {
    border: none; background: none; text-align: left;
    padding: 5px 10px; font-size: 12px; color: #3c4457; cursor: pointer;
    border-radius: 4px; white-space: nowrap;
  }
  .emu-menu button:hover { background: #eef3fc; color: #2a5aa8; }
  .emu-menu button.current { color: #2a5aa8; font-weight: 600; }
  .model-id { font-size: 12.5px; }
  .model-display { font-size: 12px; }
  .empty { text-align: center; color: #8a93a5; padding: 40px 0; }
  footer {
    display: flex; justify-content: space-between; align-items: center;
    font-size: 13px; color: #3c4457;
  }
</style>
