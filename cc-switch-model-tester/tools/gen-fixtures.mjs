// 从 cc-switch SQL 导出提取代表性供应商配置，脱敏后生成解析器测试 fixture。
// 用法：node tools/gen-fixtures.mjs <备份.sql> <输出目录>
// 输出文件结构：{ app_type, provider_id, name, settings_config, meta }

import fs from 'node:fs';
import path from 'node:path';

const [,, sqlFile, outDirRaw] = process.argv;
if (!sqlFile || !outDirRaw) {
  console.error('用法: node tools/gen-fixtures.mjs <备份.sql> <输出目录>');
  process.exit(1);
}
const outDir = path.resolve(outDirRaw);

const content = fs.readFileSync(sqlFile, 'utf8');
const provLine = content.split('\n').find(l => l.startsWith('INSERT INTO "providers"'));
if (!provLine) { console.error('未找到 providers INSERT'); process.exit(1); }

// 单状态机解析 VALUES 元组（字符串感知，SQL 转义 '' -> '）
const rows = [];
{
  let d = 0, q = false, field = '';
  const fields = [];
  for (let i = provLine.indexOf('VALUES ') + 7; i < provLine.length; i++) {
    const ch = provLine[i];
    if (q) {
      if (ch === "'") { if (provLine[i + 1] === "'") { field += "'"; i++; } else q = false; }
      else field += ch;
    } else {
      if (ch === "'") { q = true; continue; }
      if (ch === '(') { d++; if (d === 1) { field = ''; fields.length = 0; continue; } }
      if (ch === ')') { d--; if (d === 0) { fields.push(field.trim()); rows.push(fields.slice()); fields.length = 0; continue; } }
      if (ch === ',' && d === 1) { fields.push(field.trim()); field = ''; continue; }
      if (d >= 1) field += ch;
    }
  }
}

const SENSITIVE_KEY_RE = /^(.*(api[_-]?key|apikey|access[_-]?token|auth[_-]?token|token|secret|password).*)$/i;
const AUTH_HEADER_KEYS = new Set(['authorization', 'x-api-key', 'x-goog-api-key', 'proxy-authorization']);

/** 递归脱敏 JSON 值（保持结构） */
function redact(v, keyHint) {
  if (typeof v === 'string') {
    const k = (keyHint ?? '').toLowerCase();
    if (AUTH_HEADER_KEYS.has(k)) {
      // 保留 "Bearer " 前缀形态
      return /^bearer\s/i.test(v) ? 'Bearer sk-test-redacted-0001' : 'sk-test-redacted-0001';
    }
    if (k === 'authheader') return /^bearer\s/i.test(v) ? 'Bearer sk-test-redacted-0001' : 'sk-test-redacted-0001';
    if (SENSITIVE_KEY_RE.test(k)) return 'sk-test-redacted-0001';
    return v;
  }
  if (Array.isArray(v)) return v.map((x) => redact(x, keyHint));
  if (v && typeof v === 'object') {
    const out = {};
    for (const [k, val] of Object.entries(v)) out[k] = redact(val, k);
    return out;
  }
  return v;
}

// 选定的 fixture 对象（覆盖各解析分支）
const SELECTION = [
  // Claude: 普通 anthropic / 自定义 headers / openai_chat / openai_responses
  { match: (r) => r[0] === '2e9a00c2-b270-4904-8958-29365b8d848d', file: 'claude-anthropic-kimi.json' },
  { match: (r) => r[0] === 'universal-claude-7166e17b-deaa-4140-9b42-676df412668a', file: 'claude-anthropic-agentrouter.json' },
  { match: (r) => r[0] === 'c3e9ebd3-5e02-4040-8dd1-6c18a3b4bf3c', file: 'claude-openai-chat-jungongyi.json' },
  { match: (r) => r[0] === '无名公益站-1785301579282', file: 'claude-openai-responses-wuming.json' },
  // Codex: 官方（无 model 字段）/ 第三方 custom
  { match: (r) => r[2] === 'OpenAI Official', file: 'codex-official.json' },
  { match: (r) => r[2] === 'Agentrouter' && r[1] === 'codex', file: 'codex-agentrouter.json' },
  // Pi: anthropic-messages / openai-completions / openai-responses / authHeader+clientEmulation
  { match: (r) => r[0] === 'linxi-gongyizhan', file: 'pi-anthropic-messages-linxi.json' },
  { match: (r) => r[0] === 'tokenrhythm', file: 'pi-openai-completions-tokenrhythm.json' },
  { match: (r) => r[0] === 'seekai', file: 'pi-openai-responses-zzzcoding.json' },
  { match: (r) => r[0] === 'anyrouter' && r[1] === 'pi', file: 'pi-authheader-clientemulation.json' },
];

fs.mkdirSync(outDir, { recursive: true });
let count = 0;
for (const sel of SELECTION) {
  const row = rows.find(sel.match);
  if (!row) { console.error(`未找到: ${sel.file}`); continue; }
  const [id, app, name, settings, , , , , , , , meta] = row;
  let sc, mt;
  try {
    sc = JSON.parse(settings);
    mt = JSON.parse(meta || '{}');
  } catch (e) {
    console.error(`${sel.file}: JSON 解析失败 ${e.message}`); continue;
  }
  const fixture = {
    app_type: app,
    provider_id: id,
    name,
    settings_config: JSON.stringify(redact(sc, null)),
    meta: JSON.stringify(redact(mt, null)),
  };
  fs.writeFileSync(path.join(outDir, sel.file), JSON.stringify(fixture, null, 2));
  console.log(`生成 ${sel.file} (${app} / ${name})`);
  count++;
}
console.log(`完成，共 ${count} 个 fixture -> ${outDir}`);
