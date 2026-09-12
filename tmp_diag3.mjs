// 临时诊断 3：间歇性验证——同一仿真请求连发 12 次统计（不入仓库）
import { DatabaseSync } from 'node:sqlite';
import { createHash, randomUUID } from 'node:crypto';
import { hostname } from 'node:os';
import { homedir } from 'node:os';
import { join } from 'node:path';

const db = new DatabaseSync(join(homedir(), '.cc-switch', 'cc-switch.db'), { readOnly: true });
const rows = db.prepare("SELECT settings_config FROM providers WHERE app_type='claude'").all();
const env = JSON.parse(rows.find((r) => (r.settings_config ?? '').includes('zzzcoding')).settings_config).env;
const baseUrl = env.ANTHROPIC_BASE_URL.replace(/\/+$/, '');
const token = env.ANTHROPIC_AUTH_TOKEN;

const deviceId = createHash('sha256').update(`pi-client-emulation:${hostname()}`).digest('hex');
const identity = 'You are a Claude agent, built on Anthropic' + String.fromCharCode(39) + 's Claude Agent SDK.';
const headers = {
  'content-type': 'application/json',
  authorization: `Bearer ${token}`,
  'user-agent': 'claude-cli/2.1.252 (external, sdk-cli)',
  'x-app': 'cli',
  'anthropic-beta': 'claude-code-20250219',
  'anthropic-version': '2023-06-01',
  'anthropic-dangerous-direct-browser-access': 'true',
};

let ok = 0, fail = 0;
for (let i = 1; i <= 12; i++) {
  const body = {
    model: 'claude-opus-5[1M]',
    max_tokens: 64,
    stream: false,
    system: [{ type: 'text', text: identity }, { type: 'text', text: '请回复 pong' }],
    messages: [{ role: 'user', content: 'hi' }],
    metadata: { user_id: JSON.stringify({ device_id: deviceId, account_uuid: '', session_id: randomUUID() }) },
  };
  try {
    const res = await fetch(baseUrl + '/v1/messages', { method: 'POST', headers, body: JSON.stringify(body) });
    const text = await res.text();
    const tag = res.status === 200 ? '✅200' : `❌${res.status}`;
    if (res.status === 200) ok++; else fail++;
    console.log(`#${String(i).padStart(2)} ${tag} ${res.status !== 200 ? text.slice(0, 90).replace(/\s+/g, ' ') : text.slice(8, 60)}`);
  } catch (e) {
    fail++;
    console.log(`#${String(i).padStart(2)} ❌网络错误 ${String(e).slice(0, 80)}`);
  }
  await new Promise((r) => setTimeout(r, 800));
}
console.log(`\n结果：${ok} 成功 / ${fail} 失败（共 12）`);
db.close();
