<script lang="ts">
  import { onMount } from 'svelte';
  import {
    buildInput,
    cancelTest,
    errText,
    getRunStatus,
    listenRunEvents,
    pauseTest,
    previewTest,
    resumeTest,
    startTest,
    type AttemptFinished,
    type DedupPreview,
    type ModelSummary,
    type TestMode,
  } from '../lib/testRunner';
  import type { ProviderCatalogView, TabAppType } from '../lib/types';
  import { selectionKey } from '../lib/types';

  let { app, catalog, selected } = $props<{
    app: TabAppType;
    catalog: () => ProviderCatalogView[];
    selected: () => Set<string>;
  }>();

  // ==================== 测试参数（人工修改后持久保存，不恢复默认） ====================
  let attemptsPerModel = $state(3);
  let timeoutSeconds = $state(60);
  let mode = $state<TestMode>('non_streaming');
  let globalConcurrency = $state(10);
  let providerConcurrency = $state(1);
  let testAllCandidateEndpoints = $state(false);
  let confirmHighConcurrency = $state(false);

  const PARAMS_KEY = 'tester.params.v1';
  function clampNum(v: unknown, min: number, max: number, fallback: number): number {
    const n = Number(v);
    if (!Number.isFinite(n)) return fallback;
    return Math.min(max, Math.max(min, Math.round(n)));
  }

  // 启动时恢复上次使用的参数
  try {
    const raw = localStorage.getItem(PARAMS_KEY);
    if (raw) {
      const p = JSON.parse(raw) as Record<string, unknown>;
      attemptsPerModel = clampNum(p.attemptsPerModel, 1, 20, 3);
      timeoutSeconds = clampNum(p.timeoutSeconds, 5, 600, 60);
      mode = p.mode === 'streaming' ? 'streaming' : 'non_streaming';
      globalConcurrency = clampNum(p.globalConcurrency, 1, 50, 10);
      providerConcurrency = clampNum(p.providerConcurrency, 1, 10, 1);
      testAllCandidateEndpoints = p.testAllCandidateEndpoints === true;
    }
  } catch {
    // 损坏的持久化数据按默认值处理
  }

  // 任一参数变化即保存
  $effect(() => {
    const data = JSON.stringify({
      attemptsPerModel,
      timeoutSeconds,
      mode,
      globalConcurrency,
      providerConcurrency,
      testAllCandidateEndpoints,
    });
    try {
      localStorage.setItem(PARAMS_KEY, data);
    } catch {
      // 存储不可用时忽略
    }
  });

  // ==================== 运行状态 ====================
  type AttemptRow = AttemptFinished & { uid: number };
  let running = $state(false);
  let runId = $state<string | null>(null);
  let paused = $state(false);
  let progress = $state({ completed: 0, total: 0 });
  let attempts = $state<AttemptRow[]>([]);
  let panelError = $state<string | null>(null);
  let info = $state<string | null>(null);
  let uidSeq = 0;
  let pendingPreview = $state<DedupPreview | null>(null);
  /** 当前标签页的选中目标数（selection key 带 app 前缀，天然隔离其他标签页） */
  const selectedCount = $derived(
    (catalog() as ProviderCatalogView[]).reduce(
      (acc: number, p: ProviderCatalogView) => {
        let n = acc;
        for (const m of p.models) if (selected().has(selectionKey(app, p.id, m.modelId))) n += 1;
        return n;
      },
      0,
    ),
  );

  // ==================== 列定义 ====================
  type TableKey = 'result' | 'detail';
  interface ColDef {
    key: string;
    label: string;
    width: number;
    filterable?: boolean;
  }

  const RESULT_COLS: ColDef[] = [
    { key: 'provider', label: '供应商', width: 150, filterable: true },
    { key: 'model', label: '模型', width: 200, filterable: true },
    { key: 'endpoint', label: '端点', width: 200 },
    { key: 'protocol', label: '协议', width: 135 },
    { key: 'result', label: '结果', width: 95, filterable: true },
    { key: 'stability', label: '稳定性', width: 100, filterable: true },
    { key: 'sent', label: '已发/成功', width: 95 },
    { key: 'rate', label: '成功率', width: 80 },
    { key: 'avg', label: '平均耗时', width: 95 },
    { key: 'sources', label: '来源', width: 90 },
  ];

  const DETAIL_COLS: ColDef[] = [
    { key: 'no', label: '第几次', width: 70 },
    { key: 'provider', label: '供应商', width: 140, filterable: true },
    { key: 'model', label: '模型', width: 185, filterable: true },
    { key: 'key', label: 'API Key', width: 90, filterable: true },
    { key: 'result', label: '结果', width: 115, filterable: true },
    { key: 'http', label: 'HTTP', width: 70 },
    { key: 'ms', label: '耗时', width: 80 },
    { key: 'msg', label: '说明', width: 340 },
  ];

  const colsOf = (t: TableKey): ColDef[] => (t === 'result' ? RESULT_COLS : DETAIL_COLS);
  // 所有列均可筛选（有筛选内容时漏斗变深灰）

  // ==================== 表格交互状态 ====================
  interface ColFilter {
    text: string;
    values: Set<string>;
  }
  type SortState = { key: string; dir: 1 | -1 } | null;

  let resultSort = $state<SortState>(null);
  let detailSort = $state<SortState>(null);
  let resultFilters = $state<Record<string, ColFilter>>({});
  let detailFilters = $state<Record<string, ColFilter>>({});
  let visible = $state<Record<TableKey, Record<string, boolean>>>({
    result: Object.fromEntries(RESULT_COLS.map((c) => [c.key, true])),
    detail: Object.fromEntries(DETAIL_COLS.map((c) => [c.key, true])),
  });
  let widths = $state<Record<TableKey, Record<string, number>>>({
    result: Object.fromEntries(RESULT_COLS.map((c) => [c.key, c.width])),
    detail: Object.fromEntries(DETAIL_COLS.map((c) => [c.key, c.width])),
  });
  let openPop = $state<{
    table: TableKey;
    kind: 'filter' | 'cols';
    key?: string;
    anchor: { left: number; top: number };
  } | null>(null);
  let resizing = $state<{ table: TableKey; key: string; startX: number; startW: number } | null>(null);

  /** 点击测试结果行后展开的分组（null = 明细区收起） */
  let selectedGroup = $state<string | null>(null);
  /** 明细区高度：null = 自动填满到底部；拖动后为固定像素 */
  let detailHeightPx = $state<number | null>(null);
  let detailAreaH = $state(0);
  let detailResizing = $state<{ startY: number; startH: number } | null>(null);
  /** 点击明细行弹出的详情浮窗 */
  let detailModal = $state<AttemptRow | null>(null);

  function filtersOf(t: TableKey) {
    return t === 'result' ? resultFilters : detailFilters;
  }
  function sortOf(t: TableKey) {
    return t === 'result' ? resultSort : detailSort;
  }
  function setSort(t: TableKey, s: SortState) {
    if (t === 'result') resultSort = s;
    else detailSort = s;
  }
  function setFilters(t: TableKey, f: Record<string, ColFilter>) {
    if (t === 'result') resultFilters = f;
    else detailFilters = f;
  }

  function visibleCols(t: TableKey): ColDef[] {
    return colsOf(t).filter((c) => visible[t][c.key]);
  }

  /** 表格总宽 = 可见列宽之和：拖动某列时其他列宽度保持不变（仅总宽变化，出现横向滚动） */
  function tableWidth(t: TableKey): number {
    return visibleCols(t).reduce((acc, c) => acc + (widths[t][c.key] ?? 100), 0);
  }

  // ==================== 取值函数 ====================
  type CellMap = Record<string, string | number | null>;

  function resultCells(s: ModelSummary): CellMap {
    return {
      provider: s.providerName,
      model: s.modelId,
      endpoint: s.endpointDisplay,
      protocol: protocolLabel(s.protocol),
      result: resultLabel(s),
      stability: stabilityLabel(s.stability),
      sent: `${s.attemptsSent}/${s.successCount}`,
      rate: s.successRate === null ? null : Math.round(s.successRate * 100),
      avg: s.avgTotalMs,
      sources: s.duplicateSourceCount > 1 ? `${s.duplicateSourceCount} 个来源` : null,
    };
  }

  function detailCells(a: AttemptRow): CellMap {
    return {
      no: a.attemptNo,
      provider: a.providerName,
      model: a.modelId,
      key: a.credentialHint ?? '—',
      result: a.status === 'success' ? '成功' : categoryLabel(a.category),
      http: a.httpStatus,
      ms: a.totalMs,
      msg: a.errorSummary ?? ((a.responseSummary ?? '').slice(0, 90) || categoryLabel(a.category)),
    };
  }

  // ==================== 实时结果汇总 ====================
  function computeSummaries(list: AttemptRow[]): ModelSummary[] {
    interface Acc {
      s: ModelSummary;
      avg: { sum: number; n: number };
      fb: { sum: number; n: number };
    }
    const map = new Map<string, Acc>();
    for (const a of list) {
      const key = `${a.providerId}|${a.modelId}|${a.endpointDisplay}|${a.protocol}`;
      let acc = map.get(key);
      if (!acc) {
        acc = {
          s: {
            providerId: a.providerId,
            providerName: a.providerName,
            modelId: a.modelId,
            endpointDisplay: a.endpointDisplay,
            protocol: a.protocol,
            attemptsSent: 0,
            successCount: 0,
            stability: 'incomplete',
            successRate: null,
            avgTotalMs: null,
            avgFirstByteMs: null,
            duplicateSourceCount: a.duplicateSourceCount,
            sourceRefs: a.sourceRefs,
          },
          avg: { sum: 0, n: 0 },
          fb: { sum: 0, n: 0 },
        };
        map.set(key, acc);
      }
      const isCancelled = a.category === 'cancelled';
      if (!isCancelled) {
        acc.s.attemptsSent += 1;
        if (a.status === 'success') acc.s.successCount += 1;
        acc.avg.sum += a.totalMs;
        acc.avg.n += 1;
        if (a.firstByteMs !== null) {
          acc.fb.sum += a.firstByteMs;
          acc.fb.n += 1;
        }
      }
      acc.s.duplicateSourceCount = Math.max(acc.s.duplicateSourceCount, a.duplicateSourceCount);
    }
    const out: ModelSummary[] = [];
    for (const acc of map.values()) {
      const s = acc.s;
      s.successRate = s.attemptsSent > 0 ? s.successCount / s.attemptsSent : null;
      s.avgTotalMs = acc.avg.n > 0 ? Math.round(acc.avg.sum / acc.avg.n) : null;
      s.avgFirstByteMs = acc.fb.n > 0 ? Math.round(acc.fb.sum / acc.fb.n) : null;
      if (s.attemptsSent === 0) s.stability = 'incomplete';
      else if (s.successCount === s.attemptsSent) s.stability = 'stable';
      else if (s.successCount === 0) s.stability = 'unavailable';
      else s.stability = 'unstable';
      out.push(s);
    }
    return out;
  }

  const resultRowsAll = $derived(computeSummaries(attempts));

  // ==================== 过滤 + 排序 ====================
  function passFilter(cell: string | number | null, f: ColFilter | undefined): boolean {
    if (!f) return true;
    const v = cell === null ? '' : String(cell);
    if (f.values.size > 0 && !f.values.has(v)) return false;
    const text = f.text.trim();
    if (text) {
      const words = text.toLowerCase().split(/\s+/).filter(Boolean);
      const hay = v.toLowerCase();
      if (!words.every((w) => hay.includes(w))) return false;
    }
    return true;
  }

  function compareCells(a: string | number | null, b: string | number | null): number {
    if (a === null && b === null) return 0;
    if (a === null) return 1;
    if (b === null) return -1;
    if (typeof a === 'number' && typeof b === 'number') return a - b;
    return String(a).localeCompare(String(b), 'zh');
  }

  function applyTable<T>(
    rows: T[],
    filters: Record<string, ColFilter>,
    sort: SortState,
    cellsOf: (r: T) => CellMap,
  ): T[] {
    let out = rows.filter((r) => {
      const cells = cellsOf(r);
      for (const [k, f] of Object.entries(filters)) {
        if (!passFilter(cells[k] ?? null, f)) return false;
      }
      return true;
    });
    if (sort) {
      const key = sort.key;
      const dir = sort.dir;
      out = [...out].sort((x, y) => compareCells(cellsOf(x)[key] ?? null, cellsOf(y)[key] ?? null) * dir);
    }
    return out;
  }

  const resultRows = $derived(applyTable(resultRowsAll, resultFilters, resultSort, resultCells));
  const detailRows = $derived(
    selectedGroup
      ? applyTable(
          attempts.filter((a) => groupKeyOfAttempt(a) === selectedGroup),
          detailFilters,
          detailSort,
          detailCells,
        )
      : [],
  );

  // ==================== 唯一值（筛选下拉） ====================
  function uniqueValues<T>(rows: T[], key: string, cellsOf: (r: T) => CellMap): string[] {
    const set = new Set<string>();
    for (const r of rows) {
      const v = cellsOf(r)[key];
      if (v !== null && v !== undefined && v !== '') set.add(String(v));
    }
    return [...set].sort((a, b) => a.localeCompare(b, 'zh'));
  }

  const resultUnique = $derived.by(() => {
    const m: Record<string, string[]> = {};
    for (const c of RESULT_COLS) {
      m[c.key] = uniqueValues(resultRowsAll, c.key, resultCells);
    }
    return m;
  });

  const detailUnique = $derived.by(() => {
    const m: Record<string, string[]> = {};
    for (const c of DETAIL_COLS) {
      m[c.key] = uniqueValues(attempts, c.key, detailCells);
    }
    return m;
  });

  // ==================== 交互处理 ====================
  function cycleSort(t: TableKey, key: string) {
    const cur = sortOf(t);
    if (!cur || cur.key !== key) setSort(t, { key, dir: 1 });
    else if (cur.dir === 1) setSort(t, { key, dir: -1 });
    else setSort(t, null);
  }

  function toggleFilterPop(e: MouseEvent, t: TableKey, key: string) {
    if (openPop && openPop.table === t && openPop.kind === 'filter' && openPop.key === key) {
      openPop = null;
      return;
    }
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openPop = { table: t, kind: 'filter', key, anchor: { left: rect.left, top: rect.bottom + 4 } };
  }

  function toggleColsMenu(e: MouseEvent, t: TableKey) {
    if (openPop && openPop.table === t && openPop.kind === 'cols') {
      openPop = null;
      return;
    }
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openPop = { table: t, kind: 'cols', anchor: { left: rect.right - 190, top: rect.bottom + 4 } };
  }

  function toggleColVisible(t: TableKey, key: string) {
    visible = { ...visible, [t]: { ...visible[t], [key]: !visible[t][key] } };
  }

  function ensureFilter(t: TableKey, key: string): ColFilter {
    const cur = filtersOf(t)[key];
    return cur ?? { text: '', values: new Set<string>() };
  }

  function setFilterText(t: TableKey, key: string, text: string) {
    const cur = ensureFilter(t, key);
    setFilters(t, { ...filtersOf(t), [key]: { text, values: cur.values } });
  }

  function toggleFilterValue(t: TableKey, key: string, value: string) {
    const cur = ensureFilter(t, key);
    const values = new Set(cur.values);
    if (values.has(value)) values.delete(value);
    else values.add(value);
    setFilters(t, { ...filtersOf(t), [key]: { text: cur.text, values } });
  }

  function clearFilter(t: TableKey, key: string) {
    const next = { ...filtersOf(t) };
    delete next[key];
    setFilters(t, next);
  }

  function clearAllFilters(t: TableKey) {
    setFilters(t, {});
  }

  function hasActiveFilter(t: TableKey, key: string): boolean {
    const f = filtersOf(t)[key];
    return !!f && (f.text.trim().length > 0 || f.values.size > 0);
  }

  function activeFilterCount(t: TableKey): number {
    return Object.keys(filtersOf(t)).filter((k) => hasActiveFilter(t, k)).length;
  }

  function closePopIfOutside(e: MouseEvent) {
    if (!openPop) return;
    const el = e.target as HTMLElement | null;
    if (el && (el.closest('.popover') || el.closest('.pop-trigger'))) return;
    openPop = null;
  }

  // ==================== 明细区展开/收起（点击测试结果行） ====================
  function groupKeyOfSummary(s: ModelSummary): string {
    return `${s.providerId}|${s.modelId}|${s.endpointDisplay}|${s.protocol}`;
  }

  function groupKeyOfAttempt(a: AttemptRow): string {
    return `${a.providerId}|${a.modelId}|${a.endpointDisplay}|${a.protocol}`;
  }

  function toggleGroup(s: ModelSummary) {
    const k = groupKeyOfSummary(s);
    selectedGroup = selectedGroup === k ? null : k;
  }

  const selectedGroupTitle = $derived.by(() => {
    if (!selectedGroup) return '';
    const s = resultRowsAll.find((x) => groupKeyOfSummary(x) === selectedGroup);
    return s ? `${s.providerName} · ${s.modelId}` : '';
  });

  function startDetailResize(e: MouseEvent) {
    e.preventDefault();
    detailResizing = { startY: e.clientY, startH: detailHeightPx ?? detailAreaH ?? 300 };
  }

  function onDetailMove(e: MouseEvent) {
    if (!detailResizing) return;
    // 分隔条在明细区上方：向下拖 = 高度减少
    detailHeightPx = Math.min(1600, Math.max(150, detailResizing.startH - (e.clientY - detailResizing.startY)));
  }

  function onDetailUp() {
    detailResizing = null;
  }

  function startResize(e: MouseEvent, t: TableKey, key: string) {
    e.preventDefault();
    e.stopPropagation();
    resizing = { table: t, key, startX: e.clientX, startW: widths[t][key] ?? 100 };
  }

  function onWindowMove(e: MouseEvent) {
    if (!resizing) return;
    const w = Math.max(50, resizing.startW + (e.clientX - resizing.startX));
    widths = { ...widths, [resizing.table]: { ...widths[resizing.table], [resizing.key]: w } };
  }

  function onWindowUp() {
    resizing = null;
  }

  // ==================== 运行控制 ====================
  async function beginTest() {
    panelError = null;
    info = null;
    const input = buildInput(app, catalog(), selected(), {
      attemptsPerModel,
      timeoutSeconds,
      mode,
      globalConcurrency,
      providerConcurrency,
      testAllCandidateEndpoints,
      applyBodyOverrides: false,
    });
    if ('error' in input) {
      panelError = input.error;
      return;
    }
    try {
      const preview = await previewTest(input);
      if (preview.groups.length > 0) {
        pendingPreview = preview;
      } else {
        await doStart(preview.previewId);
      }
    } catch (e) {
      panelError = errText(e);
    }
  }

  async function doStart(previewId: string) {
    try {
      attempts = [];
      resultFilters = {};
      detailFilters = {};
      runId = await startTest(previewId);
      running = true;
      paused = false;
      const st = await getRunStatus(runId);
      progress = { completed: st.completed, total: st.total };
    } catch (e) {
      panelError = errText(e);
    }
  }

  function cancelPreview() {
    pendingPreview = null;
    info = '已取消，未发送请求';
  }

  async function onConfirmDedup() {
    const p = pendingPreview;
    pendingPreview = null;
    if (p) await doStart(p.previewId);
  }

  async function togglePause() {
    if (!runId) return;
    try {
      if (paused) {
        await resumeTest(runId);
        paused = false;
      } else {
        await pauseTest(runId);
        paused = true;
      }
    } catch (e) {
      panelError = errText(e);
    }
  }

  async function onCancel() {
    if (!runId) return;
    try {
      await cancelTest(runId);
    } catch (e) {
      panelError = errText(e);
    }
  }

  $effect(() => {
    if (globalConcurrency > 10 && !confirmHighConcurrency) {
      const ok = window.confirm(
        `全局并发 ${globalConcurrency} 超过默认值 10：\n更高的并发可能触发供应商限流或风控，本工具不是压力测试器。\n确定继续吗？`,
      );
      if (!ok) globalConcurrency = 10;
      else confirmHighConcurrency = true;
    }
    if (globalConcurrency <= 10) confirmHighConcurrency = false;
  });

  let unlistenAll: import('@tauri-apps/api/event').UnlistenFn[] = [];

  onMount(() => {
    listenRunEvents({
      onAttemptFinished: (a) => {
        if (a.runId !== runId) return;
        attempts = [...attempts, { ...a, uid: uidSeq++ }];
      },
      onRunProgress: (p) => {
        if (p.runId !== runId) return;
        progress = { completed: p.completed, total: p.total };
      },
      onRunFinished: (r) => {
        if (r.runId !== runId) return;
        running = false;
        paused = false;
        if (r.status === 'failed') panelError = '运行失败（详见日志）';
      },
    }).then((un) => {
      unlistenAll = un;
    });
    return () => unlistenAll.forEach((u) => u());
  });

  const pct = $derived(progress.total > 0 ? Math.round((progress.completed / progress.total) * 100) : 0);

  // ==================== 标签映射 ====================
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

  function categoryLabel(c: string): string {
    const map: Record<string, string> = {
      success: '成功',
      network_error: '网络错误',
      authentication_failed: '认证失败',
      rate_limited: '限流',
      unavailable: '不可用',
      configuration_error: '配置错误',
      protocol_error: '协议错误',
      empty_response: '空响应',
      negative_match: '命中负面词',
      response_too_large: '响应超限',
      unsupported_authentication: '暂不支持',
      cancelled: '已取消',
    };
    return map[c] ?? c;
  }

  function stabilityLabel(s: string): string {
    const map: Record<string, string> = {
      stable: '稳定可用',
      unstable: '不稳定',
      unavailable: '不可用',
      incomplete: '未完成',
    };
    return map[s] ?? s;
  }

  function stabilityClass(s: string): string {
    switch (s) {
      case 'stable': return 'ok';
      case 'unstable': return 'warn';
      case 'unavailable': return 'err';
      default: return 'gray';
    }
  }

  function resultLabel(s: ModelSummary): string {
    if (s.attemptsSent === 0) return '未完成';
    if (s.successCount === s.attemptsSent) return '成功';
    if (s.successCount === 0) return '失败';
    return '部分成功';
  }

  function resultClass(label: string): string {
    switch (label) {
      case '成功': return 'ok';
      case '失败': return 'err';
      case '部分成功': return 'warn';
      default: return 'gray';
    }
  }
</script>

<svelte:window
  onmousemove={(e) => {
    onWindowMove(e);
    onDetailMove(e);
  }}
  onmouseup={() => {
    onWindowUp();
    onDetailUp();
  }}
  onclick={closePopIfOutside}
/>

<div class="panel" class:has-detail={selectedGroup !== null}>
  <!-- 参数区 -->
  <div class="params">
    <label>次数
      <input type="number" min="1" max="20" bind:value={attemptsPerModel} disabled={running} />
    </label>
    <label>超时（秒）
      <input type="number" min="5" max="600" step="5" bind:value={timeoutSeconds} disabled={running} title="单个请求的最大等待时间，超时判失败（避免卡住阻塞后续测试）" />
    </label>
    <label>模式
      <select bind:value={mode} disabled={running}>
        <option value="non_streaming">快速非流式</option>
        <option value="streaming">兼容性流式</option>
      </select>
    </label>
    <label>全局并发
      <input type="number" min="1" max="50" bind:value={globalConcurrency} disabled={running} />
    </label>
    <label>单供应商并发
      <input type="number" min="1" max="10" bind:value={providerConcurrency} disabled={running} />
    </label>
    <label class="chk">
      <input type="checkbox" bind:checked={testAllCandidateEndpoints} disabled={running} />
      测试全部候选端点
    </label>
    <span class="spacer"></span>
    {#if !running}
      <button class="btn primary" onclick={beginTest} disabled={selectedCount === 0}>
        开始测试（已选 {selectedCount} 个目标）
      </button>
    {:else}
      <button class="btn" onclick={togglePause}>{paused ? '继续' : '暂停'}</button>
      <button class="btn danger" onclick={onCancel}>取消</button>
    {/if}
  </div>

  {#if panelError}
    <div class="banner err-banner">{panelError}</div>
  {/if}
  {#if info}
    <div class="banner info-banner">{info}</div>
  {/if}

  {#if running || progress.total > 0}
    <div class="progress-row">
      <div class="bar"><div class="fill" style="width: {pct}%"></div></div>
      <span class="mono muted">{progress.completed}/{progress.total}（{pct}%）</span>
      {#if paused}<span class="badge warn">已暂停</span>{/if}
    </div>
  {/if}

  <!-- 去重确认对话框（文档 5.9） -->
  {#if pendingPreview}
    <div class="modal-mask">
      <div class="modal">
        <h3>发现重复测试目标</h3>
        <p class="modal-desc">
          去重前 {pendingPreview.summary.originalTargetCount} 个目标 / {pendingPreview.summary.originalAttemptCount} 次请求 →
          去重后 {pendingPreview.summary.deduplicatedTargetCount} 个目标 / {pendingPreview.summary.deduplicatedAttemptCount} 次请求
        </p>
        <ul class="dedup-list">
          <li>重复组：<b>{pendingPreview.summary.duplicateGroupCount}</b> 组</li>
          <li>被合并来源：<b>{pendingPreview.summary.originalTargetCount - pendingPreview.summary.deduplicatedTargetCount}</b> 个</li>
          <li>预计减少请求：<b>{pendingPreview.summary.removedAttemptCount}</b> 次</li>
        </ul>
        {#if pendingPreview.groups.length > 0}
          <div class="dedup-groups">
            <div class="dedup-title">重复明细（代表项只发送一次请求）：</div>
            {#each pendingPreview.groups.slice(0, 20) as g}
              <div class="dedup-item">
                <span class="mono">{g.representative.providerName} · {g.representative.modelId}</span>
                <span class="muted"> ← 合并 {g.mergedSources.length + 1} 个来源：</span>
                <span class="muted">
                  {g.mergedSources.map((s) => `${s.providerName}/${s.modelKey.split('::')[1] ?? ''}`).join('、')}
                </span>
              </div>
            {/each}
            {#if pendingPreview.groups.length > 20}
              <div class="muted">…共 {pendingPreview.groups.length} 组</div>
            {/if}
          </div>
        {/if}
        <div class="modal-actions">
          <button class="btn" onclick={cancelPreview}>取消（不发送请求）</button>
          <button class="btn primary" onclick={onConfirmDedup}>确认并开始测试</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- ==================== 测试结果（模型汇总） ==================== -->
  <div class="table-block">
    <div class="table-head">
      <h4>测试结果{resultRowsAll.length > 0 ? `（${resultRows.length}/${resultRowsAll.length}）` : ''}</h4>
      <span class="spacer"></span>
      {#if activeFilterCount('result') > 0}
        <button class="btn xs" onclick={() => clearAllFilters('result')}>清除筛选（{activeFilterCount('result')}）</button>
      {/if}
      <button class="btn xs pop-trigger" onclick={(e) => toggleColsMenu(e, 'result')}>⚙ 列</button>
      {#if openPop && openPop.table === 'result' && openPop.kind === 'cols'}
        <div class="popover cols-pop">
          {#each RESULT_COLS as c}
            <label><input type="checkbox" checked={visible.result[c.key]} onchange={() => toggleColVisible('result', c.key)} /> {c.label}</label>
          {/each}
        </div>
      {/if}
    </div>

    <div class="table-wrap">
      <table style="width:{tableWidth('result')}px">
        <thead>
          <tr>
            {#each visibleCols('result') as c (c.key)}
              <th style="width:{widths.result[c.key]}px; min-width:{widths.result[c.key]}px;">
                <div class="th-inner">
                  <span class="th-label" role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') cycleSort('result', c.key); }} onclick={() => cycleSort('result', c.key)} title="点击排序">
                    {c.label}
                    {#if resultSort && resultSort.key === c.key}
                      <span class="sort-mark">{resultSort.dir === 1 ? '▲' : '▼'}</span>
                    {/if}
                  </span>
                  {#if true}
                    <button
                      class="funnel pop-trigger {hasActiveFilter('result', c.key) ? 'on' : ''}"
                      onclick={(e) => toggleFilterPop(e, 'result', c.key)}
                      title={hasActiveFilter('result', c.key) ? '该列有筛选条件（点击查看/修改）' : '筛选'}
                    >{hasActiveFilter('result', c.key) ? '▼' : '▽'}</button>
                  {/if}
                  <button type="button" class="resizer" aria-label="调整列宽" onmousedown={(e) => startResize(e, 'result', c.key)}></button>
                </div>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each resultRows as s (s.providerId + '|' + s.modelId + '|' + s.endpointDisplay)}
            <tr class="clickable {selectedGroup === groupKeyOfSummary(s) ? 'selected' : ''}" onclick={() => toggleGroup(s)} title="点击展开/收起该组测试明细">
              {#each visibleCols('result') as c (c.key)}
                <td class="cell" style="max-width:{widths.result[c.key]}px;">
                  {#if c.key === 'result'}
                    <span class="badge {resultClass(resultCells(s).result as string)}">{resultCells(s).result}</span>
                  {:else if c.key === 'stability'}
                    <span class="badge {stabilityClass(s.stability)}">{stabilityLabel(s.stability)}</span>
                  {:else if c.key === 'rate'}
                    {resultCells(s).rate === null ? '—' : `${resultCells(s).rate}%`}
                  {:else if c.key === 'avg'}
                    {resultCells(s).avg === null ? '—' : `${resultCells(s).avg} ms`}
                  {:else if c.key === 'sources'}
                    {resultCells(s).sources ?? '—'}
                  {:else}
                    <span class="ellip" title={String(resultCells(s)[c.key] ?? '')}>{resultCells(s)[c.key] ?? '—'}</span>
                  {/if}
                </td>
              {/each}
            </tr>
          {/each}
          {#if resultRowsAll.length === 0}
            <tr><td class="empty" colspan={visibleCols('result').length}>尚无测试结果</td></tr>
          {/if}
        </tbody>
      </table>
    </div>
  </div>

  <!-- ==================== 测试明细（点击测试结果行展开/收起，高度可拖动） ==================== -->
  {#if selectedGroup}
    <button type="button"
      class="detail-splitter"
      aria-label="拖动调整明细区高度"
      onmousedown={startDetailResize}
      title="拖动调整明细区高度"
    ></button>
    <div
      class="table-block detail-area"
      style={detailHeightPx ? `height:${detailHeightPx}px; flex:none;` : 'flex:1 1 auto; min-height:180px;'}
      bind:clientHeight={detailAreaH}
    >
      <div class="table-head">
        <h4>测试明细 — {selectedGroupTitle}{detailRows.length > 0 ? `（${detailRows.length} 条）` : ''}</h4>
        <span class="muted hint">点击任意行可展开完整提示词与响应摘要</span>
        <span class="spacer"></span>
        {#if activeFilterCount('detail') > 0}
          <button class="btn xs" onclick={() => clearAllFilters('detail')}>清除筛选（{activeFilterCount('detail')}）</button>
        {/if}
        <button class="btn xs pop-trigger" onclick={(e) => toggleColsMenu(e, 'detail')}>⚙ 列</button>
        <button class="btn xs" onclick={() => (selectedGroup = null)}>收起 ✕</button>
        {#if openPop && openPop.table === 'detail' && openPop.kind === 'cols'}
          <div class="popover cols-pop">
            {#each DETAIL_COLS as c}
              <label><input type="checkbox" checked={visible.detail[c.key]} onchange={() => toggleColVisible('detail', c.key)} /> {c.label}</label>
            {/each}
          </div>
        {/if}
      </div>

    <div class="table-wrap">
      <table style="width:{tableWidth('detail')}px">
        <thead>
          <tr>
            {#each visibleCols('detail') as c (c.key)}
              <th style="width:{widths.detail[c.key]}px; min-width:{widths.detail[c.key]}px;">
                <div class="th-inner">
                  <span class="th-label" role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') cycleSort('detail', c.key); }} onclick={() => cycleSort('detail', c.key)} title="点击排序">
                    {c.label}
                    {#if detailSort && detailSort.key === c.key}
                      <span class="sort-mark">{detailSort.dir === 1 ? '▲' : '▼'}</span>
                    {/if}
                  </span>
                  {#if true}
                    <button
                      class="funnel pop-trigger {hasActiveFilter('detail', c.key) ? 'on' : ''}"
                      onclick={(e) => toggleFilterPop(e, 'detail', c.key)}
                      title={hasActiveFilter('detail', c.key) ? '该列有筛选条件（点击查看/修改）' : '筛选'}
                    >{hasActiveFilter('detail', c.key) ? '▼' : '▽'}</button>
                  {/if}
                  <button type="button" class="resizer" aria-label="调整列宽" onmousedown={(e) => startResize(e, 'detail', c.key)}></button>
                </div>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each detailRows as a (a.uid)}
            <tr class="clickable" onclick={() => (detailModal = a)} title="点击查看完整提示词与响应">
              {#each visibleCols('detail') as c (c.key)}
                <td class="cell" style="max-width:{widths.detail[c.key]}px;">
                  {#if c.key === 'result'}
                    <span class="badge {a.status === 'success' ? 'ok' : 'err'}">{detailCells(a).result}</span>
                  {:else if c.key === 'no'}
                    {a.attemptNo}
                  {:else if c.key === 'http'}
                    {a.httpStatus ?? '—'}
                  {:else if c.key === 'ms'}
                    {a.totalMs} ms
                  {:else if c.key === 'msg'}
                    <span class="ellip" title={String(detailCells(a).msg ?? '')}>{detailCells(a).msg ?? '—'}</span>
                  {:else}
                    <span class="ellip" title={String(detailCells(a)[c.key] ?? '')}>{detailCells(a)[c.key] ?? '—'}</span>
                  {/if}
                </td>
              {/each}
            </tr>
          {/each}
          {#if detailRows.length === 0}
            <tr><td class="empty" colspan={visibleCols('detail').length}>该组暂无明细</td></tr>
          {/if}
        </tbody>
      </table>
    </div>
  </div>
  {/if}

  <!-- 根级 fixed 筛选弹窗（不受表格滚动容器裁剪，左对齐到漏斗按钮） -->
  {#if openPop && openPop.kind === 'filter'}
    <div class="popover filter-pop fixed-pop" style="left:{openPop.anchor.left}px; top:{openPop.anchor.top}px">
      {#if openPop.table === 'result'}
        <input
          class="filter-text"
          type="text"
          placeholder="输入关键词（空格分隔＝同时满足）"
          value={filtersOf('result')[openPop.key!]?.text ?? ''}
          oninput={(e) => setFilterText('result', openPop!.key!, e.currentTarget.value)}
        />
        <div class="filter-values">
          {#each resultUnique[openPop.key!] ?? [] as v}
            <label>
              <input
                type="checkbox"
                checked={filtersOf('result')[openPop!.key!]?.values.has(v) ?? false}
                onchange={() => toggleFilterValue('result', openPop!.key!, v)}
              />
              <span class="filter-value-text">{v}</span>
            </label>
          {/each}
        </div>
        <div class="filter-actions">
          <button class="btn xs" onclick={() => clearFilter('result', openPop!.key!)}>清除本列</button>
        </div>
      {:else}
        <input
          class="filter-text"
          type="text"
          placeholder="输入关键词（空格分隔＝同时满足）"
          value={filtersOf('detail')[openPop.key!]?.text ?? ''}
          oninput={(e) => setFilterText('detail', openPop!.key!, e.currentTarget.value)}
        />
        <div class="filter-values">
          {#each detailUnique[openPop.key!] ?? [] as v}
            <label>
              <input
                type="checkbox"
                checked={filtersOf('detail')[openPop!.key!]?.values.has(v) ?? false}
                onchange={() => toggleFilterValue('detail', openPop!.key!, v)}
              />
              <span class="filter-value-text">{v}</span>
            </label>
          {/each}
        </div>
        <div class="filter-actions">
          <button class="btn xs" onclick={() => clearFilter('detail', openPop!.key!)}>清除本列</button>
        </div>
      {/if}
    </div>
  {/if}
  <!-- 明细详情浮窗 -->
  {#if detailModal}
    <div
      class="modal-mask"
      role="presentation"
      onclick={(e) => {
        if ((e.target as HTMLElement).classList.contains('modal-mask')) detailModal = null;
      }}
    >
      <div class="modal detail-modal">
        <div class="modal-head">
          <h3>
            {detailModal.providerName} · {detailModal.modelId}
            <span class="muted">第 {detailModal.attemptNo} 次</span>
          </h3>
          <button class="btn xs" onclick={() => (detailModal = null)}>关闭 ✕</button>
        </div>
        <div class="expand-box">
          <div class="expand-item">
            <span class="expand-label">结果 / HTTP / 耗时</span>
            <div class="expand-body">
              {detailModal.status === 'success' ? '成功' : categoryLabel(detailModal.category)}
              · HTTP {detailModal.httpStatus ?? '—'}
              · 首字节 {detailModal.firstByteMs ?? '—'} ms · 总耗时 {detailModal.totalMs} ms
              · API Key {detailModal.credentialHint ?? '—'}
            </div>
          </div>
          <div class="expand-item">
            <span class="expand-label">完整发送提示词</span>
            <div class="expand-body mono">{detailModal.promptText || '—'}</div>
          </div>
          <div class="expand-item">
            <span class="expand-label">响应摘要{detailModal.responseChars > 0 ? `（${detailModal.responseChars} 字符）` : ''}</span>
            <div class="expand-body">{detailModal.responseSummary ?? '完整响应未持久化'}</div>
          </div>
          {#if detailModal.errorSummary}
            <div class="expand-item">
              <span class="expand-label">错误信息</span>
              <div class="expand-body err-text">{detailModal.errorSummary}</div>
            </div>
          {/if}
          <div class="expand-item">
            <span class="expand-label">命中规则</span>
            <div class="expand-body">{detailModal.matchedRule ?? '—'}</div>
          </div>
          {#if detailModal.duplicateSourceCount > 1}
            <div class="expand-item">
              <span class="expand-label">代表来源（{detailModal.duplicateSourceCount} 个）</span>
              <div class="expand-body">
                {detailModal.sourceRefs.map((s) => `${s.app} / ${s.providerName} / ${s.modelKey}`).join('；')}
              </div>
            </div>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .panel { display: flex; flex-direction: column; gap: 10px; min-height: 0; height: 100%; }
  .params { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .params label { display: flex; align-items: center; gap: 5px; font-size: 13px; color: #3c4457; }
  .params input[type='number'], .params select {
    border: 1px solid #c9cfda; border-radius: 5px; padding: 4px 8px; font-size: 13px; width: 90px;
  }
  .params input[type='checkbox'] { width: 14px; height: 14px; }
  .chk { user-select: none; }
  .spacer { flex: 1; }
  .btn { border: 1px solid #c9cfda; background: #fff; border-radius: 6px; padding: 6px 16px; cursor: pointer; font-size: 13px; }
  .btn.xs { padding: 2px 9px; font-size: 12px; }
  .btn.primary { background: #3b6ef6; border-color: #3b6ef6; color: #fff; }
  .btn.primary:hover:not(:disabled) { background: #2f5bd8; }
  .btn.danger { background: #fff; color: #b03030; border-color: #e0a0a0; }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .banner { border-radius: 6px; padding: 8px 12px; font-size: 13px; }
  .err-banner { background: #fbe0e0; color: #b03030; }
  .info-banner { background: #e4edfb; color: #2a5aa8; }
  .progress-row { display: flex; align-items: center; gap: 10px; }
  .bar { flex: 1; height: 8px; background: #e8ebf0; border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: #3b6ef6; transition: width 0.2s; }

  /* 弹窗 */
  .modal-mask {
    position: fixed; inset: 0; background: rgba(24, 28, 36, 0.35);
    display: flex; align-items: center; justify-content: center; z-index: 50;
  }
  .modal {
    background: #fff; border-radius: 10px; padding: 18px 20px; max-width: 720px;
    max-height: 78vh; overflow: auto; box-shadow: 0 12px 40px rgba(0, 0, 0, 0.2);
  }
  .modal h3 { margin: 0 0 8px; font-size: 15px; }
  .modal-desc { font-size: 13px; color: #3c4457; margin: 0 0 10px; }
  .dedup-list { font-size: 13px; color: #3c4457; margin: 0 0 10px; padding-left: 18px; }
  .dedup-groups { border-top: 1px solid #eef0f4; padding-top: 8px; font-size: 12.5px; }
  .dedup-title { color: #8a93a5; margin-bottom: 6px; }
  .dedup-item { padding: 3px 0; }
  .modal-actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 14px; }

  /* 表格区块 */
  .table-block { display: flex; flex-direction: column; gap: 4px; min-height: 0; }
  .table-head { display: flex; align-items: center; gap: 8px; position: relative; }
  h4 { margin: 0; font-size: 13px; color: #3c4457; }
  .hint { font-size: 11.5px; }
  /* 结果表弹性高度：无明细区时自动填满到底部；打开明细后回到 34vh 上限 */
  .table-wrap { overflow: auto; flex: 1; min-height: 160px; border: 1px solid #eef0f4; border-radius: 6px; }
  .panel.has-detail .table-wrap { flex: none; max-height: 34vh; min-height: 120px; }
  table { border-collapse: collapse; font-size: 12.5px; table-layout: fixed; }
  th, td { text-align: left; padding: 0; border-bottom: 1px solid #f0f2f5; }
  th {
    color: #66708a; font-weight: 500; font-size: 11.5px; background: #fafbfd;
    border-right: 1px solid #e3e6ea;
    position: sticky; top: 0; z-index: 2; user-select: none; padding: 0;
  }
  .th-inner { display: flex; align-items: center; gap: 2px; padding: 6px 8px; position: relative; }
  .th-label { cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; flex: 1; }
  .th-label:hover { color: #3b6ef6; }
  .sort-mark { color: #3b6ef6; font-size: 9px; margin-left: 2px; }
  .funnel {
    border: none; background: transparent; cursor: pointer; color: #aab3c4;
    font-size: 10px; padding: 0 2px; line-height: 1;
  }
  .funnel:hover { color: #3b6ef6; }
  /* 有筛选内容的列：蓝色胶囊 + 实心三角，醒目标记 */
  .funnel.on {
    color: #fff;
    background: #3b6ef6;
    border-radius: 8px;
    padding: 0 5px;
    font-weight: 700;
  }
  .resizer {
    position: absolute; right: 0; top: 0; bottom: 0; width: 5px; cursor: col-resize;
    border: none; background: transparent; padding: 0;
  }
  .resizer:hover { background: #d6e0f7; }
  td.cell { padding: 6px 8px; overflow: hidden; }
  .clickable { cursor: pointer; }
  .clickable:hover { background: #f7f9fd; }
  .ellip { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mono { font-family: Consolas, 'Cascadia Mono', monospace; font-size: 12px; }
  .muted { color: #8a93a5; }
  .empty { text-align: center; color: #8a93a5; padding: 16px 0; }
  .badge { background: #e8ebf0; border-radius: 9px; padding: 1px 9px; font-size: 11.5px; white-space: nowrap; }
  .badge.ok { background: #d9f2e0; color: #1a7a3c; }
  .badge.err { background: #fbe0e0; color: #b03030; }
  .badge.warn { background: #fdf0d9; color: #9a6b0f; }
  .badge.gray { background: #e8ebf0; color: #6b7385; }

  /* 弹出层 */
  .popover {
    position: absolute; top: 100%; right: 0; z-index: 30; background: #fff;
    border: 1px solid #dde1e8; border-radius: 8px; box-shadow: 0 8px 24px rgba(24, 28, 36, 0.12);
    padding: 8px; min-width: 190px; text-align: left; font-weight: 400; color: #1f2430;
  }
  /* 根级 fixed 筛选弹窗：不受表格滚动容器裁剪，左对齐到漏斗按钮 */
  .popover.fixed-pop {
    position: fixed; top: auto; right: auto; z-index: 100;
    width: 260px; max-width: min(320px, 90vw);
  }
  /* 明细详情浮窗 */
  .detail-modal { width: 760px; max-width: 92vw; }
  .modal-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 10px; }
  .modal-head h3 { margin: 0; font-size: 14px; }

  /* 明细区拖动分隔条 */
  .detail-splitter {
    border: none; padding: 0;
    border: none; padding: 0;
    height: 6px; border-radius: 3px; background: #e3e6ec; cursor: row-resize; flex: none;
  }
  .detail-splitter:hover { background: #3b6ef6; }
  .detail-area .table-wrap { max-height: none; height: calc(100% - 30px); }
  tr.clickable.selected td { background: #eaf1ff; }
  .cols-pop { display: flex; flex-direction: column; gap: 4px; }
  .cols-pop label { display: flex; align-items: center; gap: 6px; font-size: 12.5px; cursor: pointer; }
  .filter-pop { min-width: 230px; max-width: 300px; }
  .filter-text {
    width: 100%; box-sizing: border-box; border: 1px solid #c9cfda; border-radius: 5px;
    padding: 4px 7px; font-size: 12px; margin-bottom: 6px;
  }
  .filter-values { max-height: 190px; overflow: auto; display: flex; flex-direction: column; gap: 2px; }
  .filter-values label { display: flex; align-items: center; gap: 6px; font-size: 12px; cursor: pointer; }
  .filter-value-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .filter-actions { display: flex; justify-content: flex-end; margin-top: 6px; }

  /* 明细展开 */
  .expand-box { display: flex; flex-direction: column; gap: 8px; }
  .expand-item { display: flex; flex-direction: column; gap: 2px; }
  .expand-label { font-size: 11.5px; color: #8a93a5; }
  .expand-body {
    font-size: 12.5px; color: #1f2430; white-space: pre-wrap; word-break: break-word;
    background: #fff; border: 1px solid #eef0f4; border-radius: 6px; padding: 6px 8px; max-height: 160px; overflow: auto;
  }
  .err-text { color: #b03030; }
</style>
