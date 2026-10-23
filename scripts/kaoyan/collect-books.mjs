// Usage:
//   node scripts/kaoyan/collect-books.mjs ecnu --year 2027 [--limit 0] [--delay 800] [--refresh]
//   node scripts/kaoyan/collect-books.mjs article --url <url> --school <name> --slug <slug> [--title <t>]
//       [--major-code 050100] [--major-name 中国语言文学] [--college 文学院] [--year 2027]
import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import { extractBookSections, htmlToText, normalizeSubject, parseMajorLabel } from './lib/extract.mjs';

const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';
const ECNU_BASE = 'https://yjszs-ks.ecnu.edu.cn';

const args = process.argv.slice(2);
const mode = args[0] && !args[0].startsWith('--') ? args[0] : 'ecnu';
const readArg = (name, fallback = null) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
};
const hasFlag = (name) => args.includes(name);

const year = String(readArg('--year', '2027'));
const delayMs = Number(readArg('--delay', '800')) || 800;
const limit = Number(readArg('--limit', '0')) || 0;
const refresh = hasFlag('--refresh');

const dataDir = path.resolve('data/kaoyan');
const booksDir = path.join(dataDir, 'books');
const cacheDir = path.join(dataDir, 'cache', 'books');

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sha1 = (text) => crypto.createHash('sha1').update(text).digest('hex');

const jars = new Map();
function jarFor(url) {
  const host = new URL(url).host;
  if (!jars.has(host)) jars.set(host, new Map());
  return jars.get(host);
}

function absorbCookies(url, response) {
  const values = typeof response.headers.getSetCookie === 'function'
    ? response.headers.getSetCookie()
    : [response.headers.get('set-cookie')].filter(Boolean);
  const jar = jarFor(url);
  for (const value of values) {
    const pair = value.split(';', 1)[0];
    const index = pair.indexOf('=');
    if (index > 0) jar.set(pair.slice(0, index).trim(), pair.slice(index + 1).trim());
  }
}

function cookieHeader(url) {
  return [...jarFor(url)].map(([name, value]) => `${name}=${value}`).join('; ');
}

function decodeBuffer(buffer, labelHint) {
  let label = labelHint;
  if (!label) {
    const head = buffer.subarray(0, 4096).toString('latin1');
    label = head.match(/<meta[^>]+charset=["']?([\w-]+)/i)?.[1];
  }
  try {
    return new TextDecoder((label || 'utf-8').toLowerCase()).decode(buffer);
  } catch {
    return buffer.toString('utf8');
  }
}

let lastNetworkAt = 0;
async function throttle() {
  const wait = delayMs - (Date.now() - lastNetworkAt);
  if (wait > 0) await sleep(wait);
  lastNetworkAt = Date.now();
}

async function fetchCached(url, { method = 'GET', form, cacheKey, type = 'html', referer } = {}) {
  const cachePath = path.join(cacheDir, `${cacheKey ?? sha1(url)}.${type === 'json' ? 'json' : 'html'}`);
  if (!refresh) {
    try {
      const cached = await fs.readFile(cachePath);
      return decodeBuffer(cached, type === 'json' ? 'utf-8' : undefined);
    } catch {
      // cache miss
    }
  }

  let lastError;
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    try {
      await throttle();
      const headers = {
        'User-Agent': UA,
        Accept: type === 'json' ? 'application/json, text/javascript, */*; q=0.01' : 'text/html,application/xhtml+xml',
      };
      if (referer) headers.Referer = referer;
      if (type === 'json') headers['X-Requested-With'] = 'XMLHttpRequest';
      const cookie = cookieHeader(url);
      if (cookie) headers.Cookie = cookie;

      let body;
      if (form) {
        headers['Content-Type'] = 'application/x-www-form-urlencoded;charset=UTF-8';
        body = new URLSearchParams(form).toString();
      }

      const response = await fetch(url, {
        method,
        headers,
        body,
        signal: AbortSignal.timeout(30000),
      });
      absorbCookies(url, response);
      const buffer = Buffer.from(await response.arrayBuffer());
      if (!response.ok) throw new Error(`HTTP ${response.status}: ${url}`);

      const charset = response.headers.get('content-type')?.match(/charset=([\w-]+)/i)?.[1];
      const text = decodeBuffer(buffer, type === 'json' ? 'utf-8' : charset);
      await fs.mkdir(cacheDir, { recursive: true });
      await fs.writeFile(cachePath, buffer);
      return text;
    } catch (error) {
      lastError = error;
      console.error(`  retry ${attempt}/3 ${url}: ${error.message}`);
      if (attempt < 3) await sleep(800 * attempt);
    }
  }
  throw lastError;
}

function groupSubjects(sections) {
  const groups = new Map();
  for (const { subjectRaw, stage, citation } of sections) {
    const raw = (subjectRaw ?? '').trim() || '未注明科目';
    const key = `${stage ?? '初试'}|${raw}`;
    let group = groups.get(key);
    if (!group) {
      const { subjectCode, subjectName } = normalizeSubject(raw);
      group = {
        stage: stage ?? '初试',
        subjectCode,
        subjectName,
        books: [],
        seen: new Set(),
      };
      groups.set(key, group);
    }
    const bookKey = [citation.title, citation.volume, citation.publisher, citation.year].join('|');
    if (group.seen.has(bookKey)) continue;
    group.seen.add(bookKey);
    group.books.push(citation);
  }
  return [...groups.values()].map(({ seen, ...rest }) => rest);
}

function summarize(programs) {
  let subjects = 0;
  let books = 0;
  let withAuthors = 0;
  let withPublisher = 0;
  let withYear = 0;
  const titles = new Set();
  for (const program of programs) {
    for (const subject of program.subjects) {
      subjects += 1;
      for (const book of subject.books) {
        books += 1;
        if (book.authors) withAuthors += 1;
        if (book.publisher) withPublisher += 1;
        if (book.year) withYear += 1;
        titles.add(book.title);
      }
    }
  }
  return {
    programs: programs.length,
    programsWithBooks: programs.filter((program) => program.subjects.length > 0).length,
    subjects,
    books,
    uniqueTitles: titles.size,
    withAuthors,
    withPublisher,
    withYear,
  };
}

async function writeResult(slug, payload) {
  await fs.mkdir(booksDir, { recursive: true });
  const file = path.join(booksDir, `${slug}-${year}.json`);
  await fs.writeFile(file, JSON.stringify(payload, null, 2));
  return file;
}

function flattenLeaves(nodes) {
  const leaves = [];
  const walk = (list) => {
    for (const node of list) {
      if (Array.isArray(node.children) && node.children.length > 0) walk(node.children);
      else leaves.push(node);
    }
  };
  walk(nodes);
  return leaves;
}

function ecnuPageUrl(leaf) {
  const base = `${ECNU_BASE}/zsml/sszsml/zsmlYjfxYjxk/${year}/${leaf.pId}/${leaf.id}`;
  return leaf.scode ? `${base}?ksfs=${encodeURIComponent(leaf.scode)}` : base;
}

async function collectEcnu() {
  const treeUrl = `${ECNU_BASE}/zsml/getTreeDataYjxk`;
  const treeText = await fetchCached(treeUrl, {
    method: 'POST',
    form: { nd: year, zslb: '1' },
    cacheKey: `ecnu-tree-${year}`,
    type: 'json',
    referer: `${ECNU_BASE}/zsml/sszsml`,
  });
  const tree = JSON.parse(treeText);
  const leaves = flattenLeaves(tree);
  const selected = limit > 0 ? leaves.slice(0, limit) : leaves;
  console.error(`ECNU ${year}: ${leaves.length} major pages, collecting ${selected.length}`);

  const programs = [];
  const errors = [];
  for (const [index, leaf] of selected.entries()) {
    try {
      const { code, name, college } = parseMajorLabel(leaf.name);
      const pageUrl = ecnuPageUrl(leaf);
      const html = await fetchCached(pageUrl, {
        cacheKey: `ecnu-${year}-${leaf.id}`,
        type: 'html',
        referer: `${ECNU_BASE}/zsml/sszsml`,
      });
      const sections = extractBookSections(htmlToText(html));
      const subjects = groupSubjects(sections);
      const bookCount = subjects.reduce((sum, subject) => sum + subject.books.length, 0);
      programs.push({ college, majorCode: code, majorName: name, pageUrl, subjects });
      console.error(`[${index + 1}/${selected.length}] ${leaf.name}: ${subjects.length} subjects, ${bookCount} books`);
    } catch (error) {
      errors.push({ id: leaf.id, name: leaf.name, error: error.message });
      console.error(`[${index + 1}/${selected.length}] ${leaf.name}: failed ${error.message}`);
    }
  }

  const stats = summarize(programs);
  const payload = {
    school: '华东师范大学',
    year: Number(year),
    collectedAt: new Date().toISOString(),
    source: {
      type: 'official-site',
      adapter: 'ecnu',
      entryUrl: `${ECNU_BASE}/zsml/sszsml`,
      treeUrl,
    },
    programs,
    stats,
    errors,
  };
  const file = await writeResult('ecnu', payload);
  console.log(JSON.stringify({ file: path.relative(process.cwd(), file), ...stats, errors: errors.length }, null, 2));
}

async function collectArticle() {
  const url = readArg('--url');
  if (!url) throw new Error('article mode requires --url');
  const slug = readArg('--slug') || sha1(url).slice(0, 10);
  const school = readArg('--school') || slug;
  const html = await fetchCached(url, { cacheKey: `article-${slug}-${year}`, type: 'html' });
  const text = htmlToText(html);
  const title = readArg('--title') || text.split('\n').map((line) => line.trim()).find(Boolean)?.slice(0, 120) || null;
  const sections = extractBookSections(text);
  const programs = [{
    college: readArg('--college'),
    majorCode: readArg('--major-code'),
    majorName: readArg('--major-name'),
    title,
    pageUrl: url,
    subjects: groupSubjects(sections),
  }];
  const stats = summarize(programs);
  const payload = {
    school,
    year: Number(year),
    collectedAt: new Date().toISOString(),
    source: {
      type: 'official-article',
      adapter: 'article',
      entryUrl: url,
      title,
    },
    programs,
    stats,
    errors: [],
  };
  const file = await writeResult(slug, payload);
  console.log(JSON.stringify({ file: path.relative(process.cwd(), file), ...stats }, null, 2));
}

async function main() {
  if (mode === 'ecnu') await collectEcnu();
  else if (mode === 'article') await collectArticle();
  else throw new Error(`Unknown mode: ${mode}. Use "ecnu" or "article".`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
