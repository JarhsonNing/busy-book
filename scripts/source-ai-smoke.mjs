// Isolated acceptance: a native fixture CLI replaces the model; marketplaces are simulated.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { once } from 'node:events';

const root = await fs.mkdtemp(path.join(os.tmpdir(), 'taoshu-source-ai-'));
const port = Number(process.env.TAOSHU_TEST_PORT || 18986);
const base = `http://127.0.0.1:${port}`;
const executable = path.join(root, 'fixture-cli' + (process.platform === 'win32' ? '.exe' : ''));
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
let server, token;
async function api(route, body, method = body === undefined ? 'GET' : 'POST', expected = 200) {
  const response = await fetch(base + '/api' + route, { method,
    headers: { 'x-kf-token': token || '', 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body) });
  const value = await response.json();
  assert.equal(response.status, expected, JSON.stringify(value));
  return value;
}
async function until(fn, label) {
  for (let n = 0; n < 200; n++) { const value = await fn(); if (value) return value; await wait(100); }
  throw Error('Timed out: ' + label);
}
async function finished(id) {
  return until(async () => { const job = await api('/commerce/jobs/' + id); return ['completed', 'partial', 'failed', 'canceled'].includes(job.state) && job; }, 'job ' + id);
}
async function answer(bookId, invalid = false) {
  const { sources } = await api('/agents/ai/books/' + bookId + '/sources');
  const good = sources.find(s => s.sku === '第5版 上册') || sources[0];
  const other = sources.filter(s => s.id !== good.id && !s.title.includes('第4版')).slice(0, 1);
  const report = { analysis: '优先核对教材书店的第5版上册。低价商品是第4版，版本不符；其余候选仍需核实运费和代发条件。',
    recommendedSourceId: invalid ? 9999999 : good.id, backupSourceIds: other.map(s => s.id),
    sources: sources.map(s => ({ sourceId: s.id, versionMatch: s.title.includes('第4版') ? 'mismatch' : s.sku ? 'match' : 'unknown',
      reason: s.title.includes('第4版') ? '第4版与目标第5版不符，排除。' : s.sku ? '已登记第5版上册，可进一步核对采购条件。' : '仅有搜索标题，ISBN、版次与册次待核实。',
      risks: [s.freight == null && '运费未知，需按收件地核实', !s.dropship && '代发能力待确认'].filter(Boolean) })) };
  await fs.writeFile(path.join(root, 'answer.json'), JSON.stringify({ type: 'result', subtype: 'success', result: JSON.stringify(report) }));
}
async function stop() {
  if (server && server.exitCode == null) { const ended = once(server, 'exit'); server.kill(); await ended; }
}
try {
  // Rust's standard library makes the fixture executable portable across macOS/Windows/Linux.
  await fs.writeFile(path.join(root, 'fixture.rs'), `use std::{io::{self, Read},fs,env,thread,time::Duration};
fn main(){let args:Vec<String>=env::args().collect();
if args.iter().any(|s|s=="--version"){println!("1.0.0");return;}
if args.iter().any(|s|s=="status"){println!("{{\\\"logged_in\\\":true}}");return;}
if args.iter().any(|s|s=="--list-models"){println!("MODEL\\nFixture");return;}
let dir=std::path::PathBuf::from(env::var("TAOSHU_SOURCE_FIXTURE").unwrap());
let mut input=String::new();io::stdin().read_to_string(&mut input).unwrap();
fs::write(dir.join("invocation.txt"),format!("{:?}\\n{}",args,input)).unwrap();
while dir.join("hold").exists(){thread::sleep(Duration::from_millis(20));}
println!("{}",fs::read_to_string(dir.join("answer.json")).unwrap());}`);
  await promisify(execFile)('rustc', [path.join(root, 'fixture.rs'), '-o', executable]);
  server = spawn(path.resolve(process.env.TAOSHU_TEST_API_BINARY || `src-tauri/target/debug/api-server${process.platform === 'win32' ? '.exe' : ''}`), [], {
    env: { ...process.env, PORT: String(port), TAOSHU_DATA: root, TAOSHU_SOURCE_FIXTURE: root,
      TAOSHU_WEB_ORIGIN: 'http://127.0.0.1:1420' }, stdio: 'ignore' });
  server.on('error', e => { console.error(e.message); });
  await until(async () => { try { const response = await fetch(base + '/api/token'); if (response.ok) { token = (await response.json()).token; return true; } } catch {} }, 'API startup');
  await api('/agents/qoder', { mode: 'local', path: executable });
  const config = await api('/agents/ai/config');
  Object.assign(config, { provider: 'qoder', model: 'Fixture', enabled: false });
  config.models.qoder = 'Fixture';
  await api('/agents/ai/config', config);
  const book = await api('/books', { title: '货源验收 · 数据库系统概论', isbn: '9787040406641', edition: '第5版', condition: '九成新', sellPrice: 35, stock: 0 });
  const endpoint = `/commerce/books/${book.id}`;
  const url = 'https://item.taobao.com/item.htm?id=123';
  await api(endpoint + '/source-links', { urls: [url, 'https://example.com/bad'] }, 'POST', 400);
  assert.equal((await api(endpoint)).sources.length, 0);
  assert.equal((await api(endpoint + '/source-links', { urls: [url + '&spm=tracking', url] })).added, 1);
  // Simulate the existing browser collector returning a previously pasted product.
  await api('/captures', { url, itemId: '123', title: '数据库系统概论 第5版 上册', shop: '教材书店', price: 12, asSource: true, sourceBookId: book.id });
  let detail = await api(endpoint);
  assert.equal(detail.sources.length, 1);
  const source = detail.sources[0];
  assert.equal(source.data.price, 12);
  await api('/commerce/sources', { ...source, bookId: book.id, data: { ...source.data, sku: '第5版 上册' } });
  await api('/commerce/sources', { bookId: book.id, data: { url: 'https://item.taobao.com/item.htm?id=124', title: '数据库系统概论 第4版', price: 1, freight: null, status: 'pending', dropship: false } });
  await answer(book.id);
  const before = await api('/books/' + book.id);
  let job = await api('/commerce/jobs', { kind: 'ai', bookIds: [book.id], task: 'sources' });
  assert.equal((await finished(job.id)).state, 'completed');
  let report = await api(`/agents/ai/books/${book.id}/sources`);
  assert.equal(report.report.sources.length, 2);
  assert.equal(report.report.stale, false);
  assert.equal(report.report.recommendedSourceId, source.id);
  assert.equal((await api('/books/' + book.id)).sellPrice, before.sellPrice);
  assert.ok((await api(endpoint)).sources.every(s => !s.primary));
  assert.equal((await api(`/agents/ai/books/${book.id}/draft`)).job, null);
  const invocation = await fs.readFile(path.join(root, 'invocation.txt'), 'utf8');
  assert.ok(invocation.includes('--tools') && invocation.includes('--strict-mcp-config'));
  assert.ok(invocation.includes('图书货源整理助手'));
  await answer(book.id, true);
  job = await api('/commerce/jobs', { kind: 'ai', bookIds: [book.id], task: 'sources' });
  assert.equal((await finished(job.id)).state, 'failed');
  assert.equal((await api(`/agents/ai/books/${book.id}/sources`)).report.jobId, report.report.jobId);
  // A source edit during inference must preserve the old report and reject the stale answer.
  await answer(book.id);
  await fs.rm(path.join(root, 'invocation.txt'));
  await fs.writeFile(path.join(root, 'hold'), '');
  job = await api('/commerce/jobs', { kind: 'ai', bookIds: [book.id], task: 'sources' });
  await until(() => fs.stat(path.join(root, 'invocation.txt')).then(() => true, () => false), 'held inference');
  await api('/commerce/sources', { ...source, bookId: book.id, data: { ...source.data, sku: '第5版 上册', freight: 3 } });
  await fs.rm(path.join(root, 'hold'));
  assert.equal((await finished(job.id)).state, 'failed');
  assert.equal((await api(`/agents/ai/books/${book.id}/sources`)).report.stale, true);
  // Search => browser callback => one frozen source task, even with auto sale drafts enabled.
  config.enabled = true; config.autoOnSources = true; config.autoOnImport = false;
  await api('/agents/ai/config', config);
  let search = await api(endpoint + '/price-search', { action: 'start', ai: { provider: 'qoder', model: 'Fixture' } });
  const requestId = search.requestId;
  await api(endpoint + '/price-search', { action: 'batch', requestId, batch: 1, items: [{ url: 'https://item.taobao.com/item.htm?id=125', title: '数据库系统概论 第5版', price: 15, shop: '备选书店' }] });
  await answer(book.id);
  search = await api(endpoint + '/price-search', { action: 'finish', requestId, reason: 'visible-end' });
  assert.ok(search.analysisJobId);
  assert.equal((await finished(search.analysisJobId)).state, 'completed');
  assert.equal((await api(endpoint + '/price-search', { action: 'finish', requestId, reason: 'visible-end' })).analysisJobId, search.analysisJobId);
  assert.ok((await api('/commerce/jobs')).every(j => j.request.task === 'sources'));
  assert.ok((await api(endpoint)).sources.every(s => !s.primary));
  assert.equal((await api(`/agents/ai/books/${book.id}/sources`)).report.stale, false);
  // Stop with no new results only stops; it must not unexpectedly spend model quota.
  const empty = await api(endpoint + '/price-search', { action: 'start', ai: { provider: 'qoder', model: 'Fixture' } });
  assert.equal((await api(endpoint + '/price-search', { action: 'cancel', requestId: empty.requestId })).analysisJobId, undefined);
  console.log('PASS: pasted links + collector enrichment, deduplication, atomic rejection, CLI source report, invalid recommendations, stale-write protection, automatic search-to-analysis, idempotent completion, no implicit primary selection or sale drafts. Marketplace and model are simulated.');
  if (process.argv.includes('--serve')) {
    console.log('FIXTURE_API: ' + base + '; FIXTURE_BOOK: ' + book.id + '; FIXTURE_DIR: ' + root);
    await new Promise(resolve => { process.once('SIGINT', resolve); process.once('SIGTERM', resolve); });
  }
} finally {
  await stop();
  assert.equal(path.dirname(root), os.tmpdir());
  await fs.rm(root, { recursive: true, force: true });
}
