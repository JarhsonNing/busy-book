// Shared host toolkit for kaoyan collection plugins (P1).
// One canonical copy of the fetch/cache/extract machinery so the CLI and every
// runtime-dropped adapter build on identical behaviour.
import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import { extractBookSections, htmlToText, normalizeSubject, parseMajorLabel } from './extract.mjs';

const UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sha1 = (text) => crypto.createHash('sha1').update(text).digest('hex');

async function atomicWrite(file, content) {
  const temporary=file+'.'+process.pid+'.tmp';
  await fs.writeFile(temporary,content);
  await fs.rename(temporary,file);
}

export function createHost(options = {}) {
  const dataDir = path.resolve(options.dataDir || 'data/kaoyan');
  const booksDir = path.join(dataDir, 'books');
  const cacheDir = path.join(dataDir, 'cache', 'books');
  const year = String(options.year || '2027');
  const delayMs = Number(options.delay) || 800;
  const limit = Number(options.limit) || 0;
  const refresh = Boolean(options.refresh);

  const jars = new Map();
  const jarFor = (url) => {
    const host = new URL(url).host;
    if (!jars.has(host)) jars.set(host, new Map());
    return jars.get(host);
  };

  function absorbCookies(url, response) {
    const values =
      typeof response.headers.getSetCookie === 'function'
        ? response.headers.getSetCookie()
        : [response.headers.get('set-cookie')].filter(Boolean);
    const jar = jarFor(url);
    for (const value of values) {
      const pair = value.split(';', 1)[0];
      const index = pair.indexOf('=');
      if (index > 0) jar.set(pair.slice(0, index).trim(), pair.slice(index + 1).trim());
    }
  }

  const cookieHeader = (url) => [...jarFor(url)].map(([name, value]) => `${name}=${value}`).join('; ');

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

  let processed = 0;
  async function report() {
    processed++;
    if (process.env.TAOSHU_PROGRESS) await fs.writeFile(process.env.TAOSHU_PROGRESS, JSON.stringify({ processed, stage: '获取官方页面' }));
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
        await report();
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

        const response = await fetch(url, { method, headers, body, signal: AbortSignal.timeout(30000) });
        absorbCookies(url, response);
        const buffer = Buffer.from(await response.arrayBuffer());
        if (!response.ok) throw new Error(`HTTP ${response.status}: ${url}`);

        const charset = response.headers.get('content-type')?.match(/charset=([\w-]+)/i)?.[1];
        const text = decodeBuffer(buffer, type === 'json' ? 'utf-8' : charset);
        await fs.mkdir(cacheDir, { recursive: true });
        await atomicWrite(cachePath, buffer);
        await report();
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
        group = { stage: stage ?? '初试', subjectCode, subjectName, books: [], seen: new Set() };
        groups.set(key, group);
      }
      const bookKey = [citation.title, citation.authors, citation.edition, citation.volume, citation.publisher, citation.year].join('|');
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
      for (const subject of program.subjects ?? []) {
        subjects += 1;
        for (const book of subject.books ?? []) {
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
      programsWithBooks: programs.filter((program) => (program.subjects ?? []).length > 0).length,
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
    await atomicWrite(file, JSON.stringify(payload, null, 2));
    return file;
  }

  return {
    dataDir,
    booksDir,
    cacheDir,
    year,
    delayMs,
    limit,
    refresh,
    fetchCached,
    groupSubjects,
    summarize,
    writeResult,
    htmlToText,
    extractBookSections,
    normalizeSubject,
    parseMajorLabel,
    sleep,
    sha1,
  };
}

export { extractBookSections, htmlToText, normalizeSubject, parseMajorLabel };
