import './lib/lifecycle.mjs';
// Generic runner for kaoyan collection plugins (P1).
// Usage: node scripts/kaoyan/run-plugin.mjs --plugin <name|path> [--year Y] [--limit N]
//        [--delay N] [--refresh] [--set key=value]...
// `--set` carries any plugin-specific field the UI collected (school, url, slug, majorCode…),
// so custom adapters declaring their own meta.fields work without editing this runner.
// A plugin is an .mjs exporting `default { meta, async run(ctx) }` where ctx = { host, args }
// and run returns { slug?, payload }. The runner writes payload to books/<slug>-<year>.json
// and prints { file, ...stats } on stdout.
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHost } from './lib/host.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url)); // scripts/kaoyan
const REPO_ROOT = path.resolve(HERE, '..', '..');
const BUILTIN_DIR = path.join(HERE, 'adapters');
const USER_DIR = path.join(process.env.TAOSHU_COLLECT_DATA || path.join(REPO_ROOT, 'data', 'kaoyan'), 'adapters');

const args = process.argv.slice(2);
const readArg = (name, fallback = null) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
};
const hasFlag = (name) => args.includes(name);

function parseSet(argv) {
  const out = {};
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] !== '--set' || index + 1 >= argv.length) continue;
    const pair = argv[index + 1];
    const eq = pair.indexOf('=');
    if (eq <= 0) continue;
    const key = pair.slice(0, eq);
    if (!/^[A-Za-z][\w]*$/.test(key)) continue;
    out[key] = pair.slice(eq + 1);
  }
  return out;
}

async function exists(p) {
  try {
    await fs.access(p);
    return true;
  } catch {
    return false;
  }
}

async function resolvePlugin(ref) {
  const candidates = [];
  if (path.isAbsolute(ref)) candidates.push(ref);
  else {
    candidates.push(path.resolve(process.cwd(), ref));
    const file = ref.endsWith('.mjs') ? ref : `${ref}.mjs`;
    candidates.push(path.join(BUILTIN_DIR, file));
    candidates.push(path.join(USER_DIR, file));
  }
  for (const candidate of candidates) if (await exists(candidate)) return candidate;
  throw new Error(`找不到插件：${ref}（查找于 ${BUILTIN_DIR} 与 ${USER_DIR}）`);
}

async function main() {
  const ref = readArg('--plugin');
  if (!ref) throw new Error('缺少 --plugin');
  const pluginPath = await resolvePlugin(ref);
  const mod = await import(pathToFileURL(pluginPath).href);
  const plugin = mod.default ?? mod.plugin;
  if (!plugin || typeof plugin.run !== 'function') {
    throw new Error(`插件 ${pluginPath} 未导出 default { run(ctx) }`);
  }

  const host = createHost({
    dataDir: process.env.TAOSHU_COLLECT_DATA,
    year: readArg('--year', '2027'),
    delay: readArg('--delay', '800'),
    limit: readArg('--limit', '0'),
    refresh: hasFlag('--refresh'),
  });

  const ctxArgs = {
    school: readArg('--school'),
    url: readArg('--url'),
    slug: readArg('--slug'),
    title: readArg('--title'),
    college: readArg('--college'),
    majorCode: readArg('--major-code'),
    majorName: readArg('--major-name'),
  };
  for (const [key, value] of Object.entries(parseSet(args))) {
    if (value !== '') ctxArgs[key] = value;
  }
  ctxArgs.adapter = plugin.meta?.key ?? path.basename(pluginPath, '.mjs');

  const result = await plugin.run({ host, args: ctxArgs });
  const payload = result?.payload ?? result;
  if (!payload || !Array.isArray(payload.programs)) {
    throw new Error('插件 run() 未返回含 programs 数组的 payload');
  }
  const slug =
    result?.slug || ctxArgs.slug || payload.slug || host.sha1(ctxArgs.url || ctxArgs.school || pluginPath).slice(0, 10);

  const file = await host.writeResult(slug, payload);
  console.log(JSON.stringify({ file: path.relative(REPO_ROOT, file), slug, ...(payload.stats ?? {}), errors: payload.errors || [] }, null, 2));
}

main().catch((error) => {
  console.error(error?.stack || String(error));
  process.exit(1);
});
