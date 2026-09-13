// 临时诊断 9：device_id 假说——用 diag 的 device_id 发 node 请求（不入仓库）
import { DatabaseSync } from 'node:sqlite';
import { randomUUID } from 'node:crypto';
import { hostname, homedir } from 'node:os';
import { join } from 'node:path';

const db = new DatabaseSync(join(homedir(), '.cc-switch', 'cc-switch.db'), { readOnly: true });
const sc = JSON.parse(db.prepare("SELECT settings_config FROM providers WHERE app_type='pi' AND id='anyrouter'").get().settings_config);
const url = sc.baseUrl.replace(/\/+$/, '') + '/v1/messages';
const ccUA = 'claude-cli/2.1.252 (external, sdk-cli)';
const identity = 'You are a Claude agent, built on Anthropic' + String.fromCharCode(39) + 's Claude Agent SDK.';

async function go(label, deviceId) {
  const body = {
    model: 'claude-opus-4-7', max_tokens: 64, stream: false,
    system: [{ type: 'text', text: identity }],
    metadata: { user_id: JSON.stringify({ device_id: deviceId, account_uuid: '', session_id: randomUUID() }) },
    messages: [{ role: 'user', content: 'hi' }],
  };
  const t0 = Date.now();
  try {
    const res = await fetch(url, {
      method: 'POST',
      headers: {
        'content-type': 'application/json', 'user-agent': ccUA, 'x-app': 'cli',
        'anthropic-version': '2023-06-01', 'anthropic-dangerous-direct-browser-access': 'true',
        'anthropic-beta': 'claude-code-20250219,context-1m-2025-08-07',
        Authorization: 'Bearer ' + sc.apiKey,
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(15000),
    });
    const text = await res.text();
    console.log(`[${label}] HTTP ${res.status} (${Date.now() - t0}ms) ${res.status !== 200 ? text.slice(0, 70).replace(/\s+/g, ' ') : 'OK'}`);
  } catch (e) {
    console.log(`[${label}] ERR ${e.message.slice(0, 50)}`);
  }
}
const diagId = 'd5114c07e694c1bbf415cc56fb8e34e0086e6069e20e45c98ed52aa511b8de28';
await go('diag的device_id     ', diagId);
await go('随机新device_id #1  ', randomUUID().replaceAll('-', '') + randomUUID().replaceAll('-', '').slice(0, 32 - 32) + '');
await go('随机新device_id #2  ', createHash2());
function createHash2() {
  return randomUUID().replaceAll('-', '') + randomUUID().replaceAll('-', '').slice(0, 8);
}
db.close();
