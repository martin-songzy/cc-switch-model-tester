#!/usr/bin/env node
// Key 泄露扫描（M6）：检查源码、exe 产物、工具日志与数据库中是否存在疑似明文 API Key。
// 用法：node tools/scan-secrets.mjs
// 退出码：0 = 未发现泄露；1 = 发现疑似泄露（输出清单供人工复核）。

import { readdirSync, readFileSync, statSync, existsSync } from 'node:fs';
import { join, extname } from 'node:path';
import { homedir } from 'node:os';

const REPO = join(import.meta.dirname, '..');

// 疑似密钥模式（长度阈值过滤常见误报）
const PATTERNS = [
  { name: 'Anthropic(sk-ant-)', re: /sk-ant-[A-Za-z0-9\-_]{20,}/g },
  { name: 'OpenAI(sk-)        ', re: /sk-[A-Za-z0-9]{25,}/g },
  { name: 'Google(AIza)       ', re: /AIza[0-9A-Za-z\-_]{35}/g },
  { name: 'Groq(gsk_)         ', re: /gsk_[A-Za-z0-9]{20,}/g },
  { name: 'DeepSeek(sk-...32) ', re: /sk-[a-f0-9]{32}/g },
];

// 这些目录/文件跳过（构建产物或已知安全位置）
const SKIP_DIRS = new Set(['node_modules', 'target', '.git', '.svelte-kit', 'build', 'dist']);
const SKIP_FILES = new Set(['package-lock.json']);

// 脱敏占位符：包含 *** 的命中视为已脱敏，不算泄露
const REDACTED = /\*\*\*/;

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name) || SKIP_FILES.has(name)) continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) yield* walk(p);
    else yield p;
  }
}

const findings = [];

function scanText(label, text) {
  for (const { name, re } of PATTERNS) {
    for (const m of text.matchAll(re)) {
      const s = m[0];
      if (REDACTED.test(s)) continue; // 已脱敏
      findings.push({ where: label, kind: name, sample: s.slice(0, 12) + '…(' + s.length + '字符)' });
    }
  }
}

function scanBinary(label, buf) {
  // 从二进制中提取可打印 ASCII 串（≥16 字符）后按文本扫描
  const strs = buf.toString('latin1').match(/[\x20-\x7e]{16,}/g) ?? [];
  for (const s of strs) scanText(label, s);
}

// 1. 仓库源码
for (const f of walk(REPO)) {
  const ext = extname(f);
  if (['.png', '.ico', '.exe', '.dll', '.woff2'].includes(ext)) continue;
  try {
    scanText(f.replace(REPO + '\\', '').replace(REPO + '/', ''), readFileSync(f, 'utf8'));
  } catch { /* 跳过不可读文件 */ }
}

// 2. 便携版 exe 产物
const distExe = join(REPO, 'dist', 'cc-switch-model-tester.exe');
if (existsSync(distExe)) scanBinary('dist/cc-switch-model-tester.exe', readFileSync(distExe));

// 3. 工具自身数据目录（日志 + SQLite 数据库）
const dataDir = join(process.env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local'), 'CcSwitchModelTester');
if (existsSync(dataDir)) {
  for (const f of walk(dataDir)) {
    const buf = readFileSync(f);
    scanBinary('数据目录:' + f.replace(dataDir, ''), buf);
  }
} else {
  console.log('（未找到工具数据目录，跳过：' + dataDir + '）');
}

if (findings.length === 0) {
  console.log('✅ 未发现疑似明文 API Key（源码 / exe 产物 / 日志 / 工具数据库）。');
  process.exit(0);
}

console.log(`⚠️ 发现 ${findings.length} 处疑似泄露，请人工复核：`);
for (const f of findings) console.log(`  [${f.kind}] ${f.where} → ${f.sample}`);
process.exit(1);
