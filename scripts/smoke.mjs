import fs from 'node:fs';
import path from 'node:path';

const base = process.env.BASE || 'http://127.0.0.1:8901';
const dataDir = process.env.DATA || '.smoke-data';
let token = '';
const tokenFile = path.join(dataDir, '.token');
if (fs.existsSync(tokenFile)) {
  token = fs.readFileSync(tokenFile, 'utf8').trim();
} else {
  const response = await fetch(`${base}/api/token`);
  if (!response.ok) throw new Error(`无法获取 token：HTTP ${response.status}`);
  token = (await response.json()).token;
}

async function call(method, url, { body, headers = {}, raw = false } = {}) {
  const response = await fetch(base + url, {
    method,
    headers: {
      'x-kf-token': token,
      ...(body && !raw ? { 'content-type': 'application/json' } : {}),
      ...(raw ? { 'content-type': 'image/png' } : {}),
      ...headers,
    },
    body: body ? (raw ? body : JSON.stringify(body)) : undefined,
  });
  const type = response.headers.get('content-type') || '';
  if (type.includes('application/json')) {
    const data = await response.json();
    if (!response.ok) throw new Error(`${method} ${url} -> ${response.status} ${JSON.stringify(data)}`);
    return data;
  }
  const buffer = Buffer.from(await response.arrayBuffer());
  if (!response.ok) throw new Error(`${method} ${url} -> ${response.status} ${buffer.toString('utf8').slice(0, 200)}`);
  return { type, buffer };
}

function check(label, condition, detail) {
  console.log(`${condition ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`);
  if (!condition) process.exitCode = 1;
}

const health = await call('GET', '/api/health');
check('health', health.ok === true, health.baseUrl);

const allowed = await call('GET', '/api/token', { headers: { origin: 'http://localhost:1420' } });
check('token for app origin', typeof allowed.token === 'string' && allowed.token.length > 10);

const tauriAllowed = await call('GET', '/api/token', {
  headers: { origin: 'http://tauri.localhost' },
});
check('token for tauri origin', typeof tauriAllowed.token === 'string' && tauriAllowed.token.length > 10);

let blocked = false;
try {
  await call('GET', '/api/token', { headers: { origin: 'https://evil.example.com' } });
} catch (error) {
  blocked = /403/.test(error.message);
}
check('token blocked for unknown origin', blocked);

let spoofedBlocked = false;
try {
  await call('GET', '/api/token', {
    headers: { origin: 'http://tauri.localhost.evil.example.com' },
  });
} catch (error) {
  spoofedBlocked = /403/.test(error.message);
}
check('token blocked for spoofed tauri origin', spoofedBlocked);

const pricing = await call('PUT', '/api/settings', {
  body: { profitRate: 15, minProfit: 5 },
});
check(
  'pricing settings saved',
  pricing.profitRate === 15 && pricing.minProfit === 5,
  JSON.stringify({ profitRate: pricing.profitRate, minProfit: pricing.minProfit })
);

const now = Date.now();
const itemId = String(now);
const capture = await call('POST', '/api/captures', {
  body: {
    itemId,
    url: `https://item.taobao.com/item.htm?id=${itemId}`,
    title: '红楼梦 人民文学出版社 曹雪芹',
    price: 12.5,
    originalPrice: 59.7,
    shop: '测试书店',
    sales: '123人付款',
    mainImage: '',
    images: [],
    specs: { 作者: '曹雪芹', 出版社: '人民文学出版社', isbn: '9787020002207', 版次: '第3版' },
    raw: {},
  },
});
check('capture creates book', capture.created === true && capture.book?.id > 0);
check('chinese title preserved', capture.book.title === '红楼梦 人民文学出版社 曹雪芹', capture.book.title);
check('spec author/publisher mapped', capture.book.author === '曹雪芹' && capture.book.publisher === '人民文学出版社');
check('isbn mapped', capture.book.isbn === '9787020002207');
check(
  'sell price = ceil(max(cost*1.15, cost+5))',
  capture.book.sellPrice === 18,
  String(capture.book.sellPrice)
);

const bookId = capture.book.id;
const icon = fs.readFileSync('src-tauri/icon-source.png');
const uploaded = await call('POST', `/api/books/${bookId}/upload?source=upload`, { body: icon, raw: true });
check('upload image', uploaded.id > 0 && uploaded.hasOrigin === true);
check('upload image primary', uploaded.isPrimary === true);

const file = await call('GET', `/api/images/${uploaded.id}/file`);
check('image file served', file.type.startsWith('image/') && file.buffer.length > 1000, `${file.type} ${file.buffer.length}B`);

const book = await call('GET', `/api/books/${bookId}`);
check('book detail images', book.images.length === 1 && book.imageCount === 1);

const processed = await call('POST', `/api/images/${uploaded.id}/processed`, {
  body: icon,
  raw: true,
});
check(
  'processed image saved',
  processed.processed === true && processed.processedUrl.includes('variant=processed')
);
const processedFile = await call('GET', `/api/images/${uploaded.id}/file?variant=processed`);
check(
  'processed image served',
  processedFile.type.startsWith('image/') && processedFile.buffer.length > 1000,
  `${processedFile.type} ${processedFile.buffer.length}B`
);

const listing = await call('POST', `/api/books/${bookId}/listing`, { body: {} });
check('listing created', listing.listing.id > 0);
check('publish url has task', /kf_listing=\d+/.test(listing.publishUrl), listing.publishUrl);
check('listing has images', listing.images.length === 1);

const payload = await call('GET', `/api/listings/${listing.listing.id}/payload`);
check('payload title', payload.title.includes('红楼梦') && payload.title.length <= 30, payload.title);
check('payload price', payload.price === 18, String(payload.price));
check('payload absolute image url', payload.images[0].url.startsWith(base), payload.images[0].url);
check('payload uses processed image', payload.images[0].url.includes('variant=processed'), payload.images[0].url);
check('payload autoSubmit', typeof payload.autoSubmit === 'boolean');

const second = await call('POST', `/api/books/${bookId}/upload?source=upload`, {
  body: icon,
  raw: true,
});
check('upload second image', second.id !== uploaded.id && second.isPrimary === false);
const primary = await call('POST', `/api/images/${second.id}/primary`);
check('set primary image', primary.isPrimary === true);
const twoImages = await call('GET', `/api/books/${bookId}`);
check(
  'primary switched',
  twoImages.images.filter((image) => image.isPrimary).length === 1 &&
    twoImages.images.find((image) => image.id === second.id)?.isPrimary === true
);

const reported = await call('POST', `/api/listings/${listing.listing.id}/report`, {
  body: { status: 'published', url: 'https://www.goofish.com/item?id=1', error: '' },
});
check('report published', reported.status === 'published');
const listed = await call('GET', `/api/books/${bookId}`);
check('book status listed', listed.status === 'listed');

const csv = await call('GET', '/api/export.csv');
check('csv export', csv.buffer.length > 30 && csv.buffer.toString('utf8').includes('红楼梦'));

const stats = await call('GET', '/api/stats');
check('stats totals', stats.total >= 1 && stats.listed >= 1, JSON.stringify(stats));

const unauthorized = await fetch(`${base}/api/stats`);
check('stats requires token', unauthorized.status === 401, String(unauthorized.status));

console.log('smoke done');
