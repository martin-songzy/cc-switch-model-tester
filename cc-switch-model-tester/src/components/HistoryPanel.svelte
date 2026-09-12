<script lang="ts">
  import { onMount } from 'svelte';
  import { historyAttempts, historyClear, historyDeleteRun, historyRuns } from '../lib/api';
  import type { HistoryAttempt, HistoryRun } from '../lib/types';

  let runs = $state<HistoryRun[]>([]);
  let errorMsg = $state('');
  let loading = $state(false);
  /** 分类过滤：全部 / claude / codex / pi */
  let appFilter = $state<'all' | string>('all');

  const APP_LABELS: Record<string, string> = {
    claude: 'Claude Code',
    codex: 'Codex',
    pi: 'Pi agent',
  };

  const filteredRuns = $derived(appFilter === 'all' ? runs : runs.filter((r) => r.appType === appFilter));
  const presentApps = $derived([...new Set(runs.map((r) => r.appType).filter(Boolean))]);

  // 当前查看的轮次（null = 列表视图）
  let openRun = $state<HistoryRun | null>(null);
  let attempts = $state<HistoryAttempt[]>([]);
  let detailRow = $state<HistoryAttempt | null>(null);

  function fmtTime(ts: number | null): string {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString('zh-CN', { hour12: false });
  }

  function fmtMs(ms: number | null): string {
    if (ms == null) return '—';
    return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
  }

  function statusLabel(s: string): string {
    if (s === 'running') return '进行中';
    if (s === 'completed') return '已完成';
    if (s === 'cancelled') return '已取消';
    if (s === 'failed') return '异常';
    return s;
  }

  function categoryLabel(c: string): string {
    const map: Record<string, string> = {
      passed: '通过',
      cancelled: '已取消',
      network_error: '网络错误',
      auth_error: '鉴权失败',
      http_error: 'HTTP错误',
      negative_rule: '负面规则',
      parse_error: '解析失败',
      empty_response: '空响应',
      error_object: '错误对象',
    };
    return map[c] ?? c;
  }

  async function reload() {
    errorMsg = '';
    try {
      runs = await historyRuns();
    } catch (e) {
      errorMsg = String(e);
    }
  }
  onMount(reload);

  async function open(run: HistoryRun) {
    loading = true;
    errorMsg = '';
    try {
      attempts = await historyAttempts(run.runId);
      openRun = run;
    } catch (e) {
      errorMsg = String(e);
    } finally {
      loading = false;
    }
  }

  async function removeRun(run: HistoryRun) {
    if (!confirm(`删除 ${fmtTime(run.startedAt)} 这一轮的测试历史？`)) return;
    try {
      await historyDeleteRun(run.runId);
      await reload();
    } catch (e) {
      errorMsg = String(e);
    }
  }

  async function clearAll() {
    if (!confirm('确定清空全部测试历史？此操作不可恢复。')) return;
    try {
      await historyClear();
      openRun = null;
      attempts = [];
      await reload();
    } catch (e) {
      errorMsg = String(e);
    }
  }

  const hasPassRate = (r: HistoryRun) => r.totalAttempts > 0;
  const passRate = (r: HistoryRun) => Math.round((r.passed / r.totalAttempts) * 100);
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape') detailRow = null;
  }}
/>

<div class="history-wrap">
  {#if openRun === null}
    <div class="toolbar">
      <h3>测试历史</h3>
      <span class="hint">自动保留最近 15 分钟</span>
      <span class="spacer"></span>
      <button class="btn sm ghost danger" onclick={clearAll} disabled={runs.length === 0}>
        清空全部
      </button>
      <button class="btn sm" onclick={reload}>刷新</button>
    </div>

    {#if presentApps.length > 0}
      <div class="app-tabs">
        <button
          class="chip"
          class:active={appFilter === 'all'}
          onclick={() => (appFilter = 'all')}
        >全部（{runs.length}）</button>
        {#each presentApps as app (app)}
          <button
            class="chip"
            class:active={appFilter === app}
            onclick={() => (appFilter = app)}
          >
            {APP_LABELS[app] ?? app}（{runs.filter((r) => r.appType === app).length}）
          </button>
        {/each}
      </div>
    {/if}

    {#if errorMsg}<div class="err">{errorMsg}</div>{/if}

    {#if runs.length === 0}
      <div class="empty">暂无测试历史——完成一轮测试后，这里会自动记录。</div>
    {:else if filteredRuns.length === 0}
      <div class="empty">该分类下暂无测试历史。</div>
    {:else}
      <div class="table-box">
        <table>
          <thead>
            <tr>
              <th>开始时间</th>
              <th>分类</th>
              <th>状态</th>
              <th>完成 / 总数</th>
              <th>通过</th>
              <th>失败</th>
              <th>通过率</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {#each filteredRuns as r (r.runId)}
              <tr>
                <td class="mono">{fmtTime(r.startedAt)}</td>
                <td>{APP_LABELS[r.appType] ?? (r.appType || '—')}</td>
                <td>{statusLabel(r.status)}</td>
                <td>{r.completedAttempts} / {r.totalAttempts}</td>
                <td class="good">{r.passed}</td>
                <td class:bad={r.failed > 0}>{r.failed}</td>
                <td>
                  {#if hasPassRate(r)}
                    <span class="rate" class:rate-high={passRate(r) >= 80} class:rate-low={passRate(r) < 40}>
                      {passRate(r)}%
                    </span>
                  {:else}
                    —
                  {/if}
                </td>
                <td class="ops">
                  <button class="btn sm" onclick={() => open(r)} disabled={loading}>查看明细</button>
                  <button class="btn sm ghost danger" onclick={() => removeRun(r)}>删除</button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {:else}
    <div class="toolbar">
      <button class="btn sm" onclick={() => (openRun = null)}>← 返回列表</button>
      <h3>{fmtTime(openRun.startedAt)} 的测试明细</h3>
      <span class="hint">
        通过 {openRun.passed} / 失败 {openRun.failed} / 共 {openRun.totalAttempts}
      </span>
      <span class="spacer"></span>
      <span class="hint">点击行查看完整响应/错误</span>
    </div>

    {#if errorMsg}<div class="err">{errorMsg}</div>{/if}

    {#if attempts.length === 0}
      <div class="empty">该轮没有留下明细记录。</div>
    {:else}
      <div class="table-box">
        <table>
          <thead>
            <tr>
              <th>供应商</th>
              <th>模型</th>
              <th>API Key</th>
              <th>端点</th>
              <th>协议</th>
              <th>结果</th>
              <th>HTTP</th>
              <th>耗时</th>
              <th>摘要 / 错误</th>
            </tr>
          </thead>
          <tbody>
            {#each attempts as a, i (i)}
              <tr class="clickable" onclick={() => (detailRow = a)}>
                <td>{a.providerName}</td>
                <td class="mono">{a.modelDisplayName || a.modelId}</td>
                <td class="mono" title="仅显示末尾几位，用于区分不同 key">{a.credentialHint ?? '—'}</td>
                <td class="mono">{a.endpointDisplay || '—'}</td>
                <td>{a.protocol}</td>
                <td>
                  {#if a.status === 'success'}
                    <span class="pill good">通过</span>
                  {:else if a.category === 'cancelled'}
                    <span class="pill muted">已取消</span>
                  {:else}
                    <span class="pill bad">{categoryLabel(a.category)}</span>
                  {/if}
                </td>
                <td class="mono">{a.httpStatus ?? '—'}</td>
                <td class="mono">{fmtMs(a.totalLatencyMs)}</td>
                <td class="sum">{a.errorSummary || a.responseSummary || '—'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}

  {#if detailRow !== null}
    <!-- 遮罩点击关闭是浮窗惯例；键盘用户可用 Esc 关闭 -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="modal-mask" role="presentation" onclick={() => (detailRow = null)}>
      <div class="modal" role="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()}>
        <div class="modal-head">
          <strong>
            {detailRow.providerName} · {detailRow.modelDisplayName || detailRow.modelId} · 第
            {detailRow.attemptNo} 次
          </strong>
          <button class="btn sm" onclick={() => (detailRow = null)}>关闭</button>
        </div>
        <div class="modal-body">
          <div class="kv"><span>时间</span><span>{fmtTime(detailRow.testedAt)}</span></div>
          <div class="kv"><span>端点</span><span class="mono">{detailRow.endpointDisplay || '—'}</span></div>
          <div class="kv">
            <span>API Key</span>
            <span class="mono">{detailRow.credentialHint ?? '—'}（仅末尾几位，用于区分）</span>
          </div>
          <div class="kv"><span>协议 / 模式</span><span>{detailRow.protocol} / {detailRow.mode}</span></div>
          <div class="kv">
            <span>结果</span>
            <span>
              {detailRow.status === 'success' ? '通过' : categoryLabel(detailRow.category)}
              {#if detailRow.httpStatus != null}
                （HTTP {detailRow.httpStatus}，耗时 {fmtMs(detailRow.totalLatencyMs)}）
              {/if}
              {#if detailRow.matchedRule}
                命中规则：{detailRow.matchedRule}
              {/if}
            </span>
          </div>
          <div class="kv"><span>提示词</span><pre>{detailRow.promptText}</pre></div>
          {#if detailRow.responseSummary}
            <div class="kv"><span>响应</span><pre>{detailRow.responseSummary}</pre></div>
          {/if}
          {#if detailRow.errorSummary}
            <div class="kv"><span>错误</span><pre class="err-pre">{detailRow.errorSummary}</pre></div>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .history-wrap {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px 20px 24px;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
    box-sizing: border-box;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  h3 {
    margin: 0;
    font-size: 14px;
    color: #3d4656;
  }
  .hint {
    font-size: 12px;
    color: #8a93a5;
  }
  .spacer {
    flex: 1;
  }
  .app-tabs {
    display: flex;
    gap: 6px;
  }
  .chip {
    border: 1px solid #dde1e8;
    background: #fff;
    border-radius: 14px;
    padding: 3px 12px;
    font-size: 12px;
    color: #5a6375;
    cursor: pointer;
  }
  .chip:hover {
    border-color: #3b6ef6;
    color: #3b6ef6;
  }
  .chip.active {
    background: #e4edfb;
    border-color: #3b6ef6;
    color: #2a5aa8;
    font-weight: 600;
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
  .btn:hover:not(:disabled) {
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
  .btn.danger:hover:not(:disabled) {
    border-color: #d64545;
    color: #d64545;
  }
  .btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .err {
    background: #fdf1f1;
    border: 1px solid #f2caca;
    color: #b03333;
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 12px;
  }
  .empty {
    color: #8a93a5;
    font-size: 13px;
    padding: 40px 0;
    text-align: center;
  }

  .table-box {
    background: #fff;
    border: 1px solid #e3e6ea;
    border-radius: 10px;
    overflow: auto;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 12px;
  }
  th,
  td {
    text-align: left;
    padding: 7px 12px;
    border-bottom: 1px solid #eef0f4;
    white-space: nowrap;
  }
  td.sum {
    max-width: 420px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  th {
    color: #5b6472;
    font-weight: 600;
    background: #fafbfd;
    border-right: 1px solid #e3e6ea;
  }
  th:last-child {
    border-right: none;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  tr.clickable {
    cursor: pointer;
  }
  tr.clickable:hover {
    background: #f5f8ff;
  }
  .mono {
    font-family: Consolas, 'Courier New', monospace;
    font-size: 11px;
    color: #5b6472;
  }
  .good {
    color: #1f9d61;
  }
  .bad {
    color: #d64545;
  }
  .rate {
    font-weight: 600;
  }
  .rate-high {
    color: #1f9d61;
  }
  .rate-low {
    color: #d64545;
  }
  .pill {
    display: inline-block;
    border-radius: 9px;
    padding: 1px 9px;
    font-size: 11px;
  }
  .pill.good {
    background: #e7f6ee;
    color: #1f9d61;
  }
  .pill.bad {
    background: #fdecec;
    color: #d64545;
  }
  .pill.muted {
    background: #eef0f4;
    color: #8a93a5;
  }
  .ops {
    display: flex;
    gap: 6px;
  }

  /* 明细浮窗 */
  .modal-mask {
    position: fixed;
    inset: 0;
    background: rgba(20, 30, 60, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }
  .modal {
    background-color: #ffffff;
    border: 1px solid #e3e6ec;
    border-radius: 12px;
    width: min(720px, 90vw);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 32px rgba(20, 30, 60, 0.25);
  }
  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 18px;
    border-bottom: 1px solid #eef0f4;
    font-size: 13px;
    color: #3d4656;
  }
  .modal-body {
    padding: 12px 18px 18px;
    overflow-y: auto;
    font-size: 12px;
    color: #3d4656;
  }
  .kv {
    display: grid;
    grid-template-columns: 84px 1fr;
    gap: 8px;
    margin-bottom: 10px;
  }
  .kv > span:first-child {
    color: #8a93a5;
  }
  pre {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    background: #f7f8fb;
    border: 1px solid #eef0f4;
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 11px;
    line-height: 1.6;
  }
  .err-pre {
    background: #fdf5f5;
    border-color: #f2dcdc;
    color: #a33;
  }
</style>
