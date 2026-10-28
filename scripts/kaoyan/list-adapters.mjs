// Lists kaoyan collection plugins for the app (P1).
// Usage: node scripts/kaoyan/list-adapters.mjs
// Imports each plugin in scripts/kaoyan/adapters (builtin) and data/kaoyan/adapters (user),
// reads its `default.meta`, and prints { adapters: [...] } on stdout. A plugin that fails to
// load still appears, tagged with an `error`, so one broken user file never blanks the list.
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url)); // scripts/kaoyan
const REPO_ROOT = path.resolve(HERE, '..', '..');
const BUILTIN_DIR = path.join(HERE, 'adapters');
const USER_DIR = path.join(process.env.TAOSHU_COLLECT_DATA || path.join(REPO_ROOT, 'data', 'kaoyan'), 'adapters');

async function listMjs(dir) {
  try {
    const names = await fs.readdir(dir);
    return names
      .filter((name) => name.endsWith('.mjs') && name !== 'list-adapters.mjs')
      .sort()
      .map((name) => path.join(dir, name));
  } catch {
    return [];
  }
}

async function describe(filePath, builtin) {
  const fallbackKey = path.basename(filePath, '.mjs');
  const base = {
    ref: filePath,
    key: fallbackKey,
    name: fallbackKey,
    description: null,
    fields: [],
    builtin,
    error: null,
  };
  try {
    const mod = await import(pathToFileURL(filePath).href);
    const plugin = mod.default ?? mod.plugin;
    if (!plugin || typeof plugin.run !== 'function') {
      return { ...base, error: '未导出 default { run(ctx) }' };
    }
    const meta = plugin.meta ?? {};
    return {
      ...base,
      key: meta.key ?? fallbackKey,
      name: meta.name ?? fallbackKey,
      description: meta.description ?? null,
      fields: Array.isArray(meta.fields) ? meta.fields : [],
    };
  } catch (error) {
    return { ...base, error: error?.message ?? String(error) };
  }
}

async function main() {
  const builtins = await listMjs(BUILTIN_DIR);
  const userFiles = await listMjs(USER_DIR);
  const described = [];
  for (const file of builtins) described.push(await describe(file, true));
  for (const file of userFiles) described.push(await describe(file, false));
  console.log(JSON.stringify({ adapters: described, builtinDir: BUILTIN_DIR, userDir: USER_DIR }, null, 2));
}

main().catch((error) => {
  console.error(error?.stack || String(error));
  process.exit(1);
});
