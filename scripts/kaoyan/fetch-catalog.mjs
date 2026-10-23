// Usage: node scripts/kaoyan/fetch-catalog.mjs --province 11 --max-units 5 --max-majors 5
import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';

const BASE = 'https://yz.chsi.com.cn';
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';
const PROVINCES = {
  11: '北京', 12: '天津', 13: '河北', 14: '山西', 15: '内蒙古',
  21: '辽宁', 22: '吉林', 23: '黑龙江',
  31: '上海', 32: '江苏', 33: '浙江', 34: '安徽', 35: '福建', 36: '江西', 37: '山东',
  41: '河南', 42: '湖北', 43: '湖南', 44: '广东', 45: '广西', 46: '海南',
  50: '重庆', 51: '四川', 52: '贵州', 53: '云南', 54: '西藏',
  61: '陕西', 62: '甘肃', 63: '青海', 64: '宁夏', 65: '新疆',
};

const args = process.argv.slice(2);
const readArg = (name, fallback) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
};

const provinceCode = String(readArg('--province', '11'));
const maxUnits = Number(readArg('--max-units', '0')) || 0;
const maxMajors = Number(readArg('--max-majors', '0')) || 0;
const delayMs = Number(readArg('--delay', '450')) || 450;
const refresh = args.includes('--refresh');

if (!PROVINCES[provinceCode]) {
  console.error(`Unknown province code: ${provinceCode}`);
  process.exit(1);
}

const dataDir = path.resolve('data/kaoyan');
const cacheDir = path.join(dataDir, 'cache');
const unitDir = path.join(dataDir, 'units');

class CookieJar {
  cookies = new Map();

  absorb(response) {
    const values = typeof response.headers.getSetCookie === 'function'
      ? response.headers.getSetCookie()
      : [response.headers.get('set-cookie')].filter(Boolean);
    for (const value of values) {
      const pair = value.split(';', 1)[0];
      const index = pair.indexOf('=');
      if (index > 0) this.cookies.set(pair.slice(0, index).trim(), pair.slice(index + 1).trim());
    }
  }

  header() {
    return [...this.cookies].map(([name, value]) => `${name}=${value}`).join('; ');
  }
}

const jar = new CookieJar();
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sha1 = (text) => crypto.createHash('sha1').update(text).digest('hex');

function encodeForm(form) {
  const body = new URLSearchParams();
  for (const [key, value] of Object.entries(form)) {
    if (value === undefined || value === null) continue;
    if (Array.isArray(value)) {
      value.forEach((item, index) => body.append(`${key}[${index}]`, String(item)));
    } else {
      body.append(key, String(value));
    }
  }
  return body.toString();
}

async function rawRequest(pathname, { method = 'POST', form, referer } = {}) {
  const headers = {
    'User-Agent': UA,
    Accept: 'application/json, text/javascript, */*; q=0.01',
    'X-Requested-With': 'XMLHttpRequest',
    Referer: referer || `${BASE}/zsml/dw.do`,
  };
  let body;
  if (form) {
    headers['Content-Type'] = 'application/x-www-form-urlencoded;charset=utf-8';
    body = encodeForm(form);
  }
  const cookie = jar.header();
  if (cookie) headers.Cookie = cookie;

  const response = await fetch(BASE + pathname, { method, headers, body });
  jar.absorb(response);
  const text = await response.text();
  if (!response.ok) throw new Error(`HTTP ${response.status}: ${text.slice(0, 160)}`);

  let data;
  try {
    data = JSON.parse(text);
  } catch {
    throw new Error(`Expected JSON: ${text.slice(0, 160)}`);
  }
  if (data.flag === false || typeof data.msg === 'string') {
    throw new Error(`API error: ${data.msg || 'unknown'}`);
  }
  return data;
}

async function request(pathname, options = {}) {
  const encoded = options.form ? encodeForm(options.form) : '';
  const cacheFile = path.join(cacheDir, `${sha1(`${options.method || 'POST'} ${pathname} ${encoded}`)}.json`);
  if (!refresh) {
    try {
      return JSON.parse(await fs.readFile(cacheFile, 'utf8'));
    } catch {
      // cache miss
    }
  }

  let lastError;
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    try {
      const data = await rawRequest(pathname, options);
      await fs.mkdir(cacheDir, { recursive: true });
      await fs.writeFile(cacheFile, JSON.stringify(data));
      await sleep(delayMs + Math.floor(Math.random() * 150));
      return data;
    } catch (error) {
      lastError = error;
      console.error(`  retry ${attempt}/3 ${pathname}: ${error.message}`);
      await sleep(700 * attempt);
    }
  }
  throw lastError;
}

async function bootstrap() {
  try {
    const response = await fetch(`${BASE}/zsml/dw.do`, {
      headers: { 'User-Agent': UA, Accept: 'text/html' },
    });
    jar.absorb(response);
    await response.text();
  } catch (error) {
    console.error(`Cookie bootstrap failed, continuing: ${error.message}`);
  }
}

async function fetchUnits() {
  const units = [];
  let start = 0;
  let curPage = 1;

  for (;;) {
    const data = await request('/zsml/rs/dws.do', {
      form: { dwdm: '', dwmc: '', ssdm: provinceCode, start, curPage, pageSize: 20 },
    });
    const message = data.msg;
    if (!Array.isArray(message?.list)) throw new Error('Unexpected unit response');
    units.push(...message.list);
    console.error(`units page ${curPage}: +${message.list.length} (${units.length}/${message.totalCount})`);
    if (!message.nextPageAvailable) break;
    start = message.startOfNextPage;
    curPage += 1;
  }
  return units;
}

function schoolFlags(unit) {
  return [
    unit.zhx === '1' ? 'zhx' : null,
    unit.syl === '1' ? 'syl' : null,
    unit.bs === '1' ? 'bs' : null,
  ].filter(Boolean);
}

async function fetchMajors(unit) {
  const majors = [];
  const flags = schoolFlags(unit);
  let start = 0;
  let curPage = 1;

  for (;;) {
    const form = {
      dwdm: unit.dwdm,
      dwmc: unit.dwmc,
      schId: unit.schId,
      zydm: '',
      zymc: '',
      xwlx: '',
      mldm: '',
      yjxkdm: '',
      xxfs: '',
      tydxs: '',
      jsggjh: '',
      jsxbjh: '',
      start,
      curPage,
      pageSize: 20,
      sign: unit.sign,
    };
    if (flags.length > 0) form.dwlxs = flags;

    const data = await request('/zsml/rs/dwzys.do', { form });
    const message = data.msg;
    if (!Array.isArray(message?.list)) throw new Error('Unexpected major response');
    majors.push(...message.list);
    if (!message.nextPageAvailable) break;
    start = message.startOfNextPage;
    curPage += 1;
  }
  return majors;
}

async function fetchDirections(unit, major) {
  const directions = [];
  let start = 0;

  for (;;) {
    const form = {
      zydm: major.zydm,
      zymc: major.zymc,
      dwdm: unit.dwdm,
      xxfs: major.mxxfs || '',
      tydxs: major.mtydxs || '',
      jsggjh: major.mjsggjh || '',
      jsxbjh: major.mjsxbjh || '',
      start,
      pageSize: 10,
      totalCount: 0,
    };
    if (Array.isArray(major.mdwlxs) && major.mdwlxs.length > 0) form.dwlxs = major.mdwlxs;

    const data = await request('/zsml/rs/yjfxs.do', { form });
    const message = data.msg;
    if (!Array.isArray(message?.list)) throw new Error('Unexpected direction response');
    directions.push(...message.list);
    if (!message.nextPageAvailable) break;
    start = message.startOfNextPage;
  }
  return directions;
}

async function collectUnit(unit) {
  const majors = await fetchMajors(unit);
  const selected = maxMajors > 0 ? majors.slice(0, maxMajors) : majors;
  const details = [];

  for (const major of selected) {
    const directions = await fetchDirections(unit, major);
    details.push({ ...major, directions });
    console.error(`  ${major.zydm} ${major.zymc}: ${directions.length} directions`);
  }

  return {
    dwdm: unit.dwdm,
    dwmc: unit.dwmc,
    schId: unit.schId,
    province: unit.szss,
    flags: { zhx: unit.zhx === '1', bs: unit.bs === '1', syl: unit.syl === '1' },
    majorCount: majors.length,
    majorsFetched: details.length,
    majors: details,
  };
}

async function main() {
  console.error(`Fetching ${PROVINCES[provinceCode]} (${provinceCode}) from yz.chsi.com.cn`);
  await bootstrap();
  const units = await fetchUnits();
  await fs.mkdir(dataDir, { recursive: true });
  await fs.writeFile(
    path.join(dataDir, `units-${provinceCode}.json`),
    JSON.stringify(units, null, 2),
  );

  const selected = maxUnits > 0 ? units.slice(0, maxUnits) : units;
  const results = [];
  const errors = [];

  for (const [index, unit] of selected.entries()) {
    console.error(`[${index + 1}/${selected.length}] ${unit.dwdm} ${unit.dwmc}`);
    try {
      const result = await collectUnit(unit);
      results.push(result);
      await fs.mkdir(unitDir, { recursive: true });
      await fs.writeFile(
        path.join(unitDir, `${unit.dwdm}.json`),
        JSON.stringify(result, null, 2),
      );
    } catch (error) {
      errors.push({ dwdm: unit.dwdm, dwmc: unit.dwmc, error: error.message });
      console.error(`  failed: ${error.message}`);
    }
  }

  const subjectCodes = new Set();
  let directionCount = 0;
  let majorCount = 0;
  for (const result of results) {
    majorCount += result.majorsFetched;
    for (const major of result.majors) {
      for (const direction of major.directions) {
        directionCount += 1;
        for (const group of direction.kskmz || []) {
          for (const key of ['km1Vo', 'km2Vo', 'km3Vo', 'km4Vo']) {
            const code = group[key]?.kskmdm;
            if (code) subjectCodes.add(code);
          }
        }
      }
    }
  }

  const summary = {
    source: `${BASE}/zsml/`,
    cycle: '2027',
    province: { code: provinceCode, name: PROVINCES[provinceCode] },
    fetchedAt: new Date().toISOString(),
    unitsTotal: units.length,
    unitsFetched: results.length,
    schoolMajorRows: majorCount,
    directions: directionCount,
    examSubjectCodes: subjectCodes.size,
    errors,
  };
  await fs.writeFile(
    path.join(dataDir, `summary-${provinceCode}.json`),
    JSON.stringify(summary, null, 2),
  );
  console.log(JSON.stringify(summary, null, 2));
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
