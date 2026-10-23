import { invoke } from '@tauri-apps/api/core';

let BASE = 'http://127.0.0.1:8899';
let token = '';

export function apiBase() {
  return BASE;
}

export function assetUrl(path) {
  if (!path) return '';
  if (/^https?:\/\//i.test(path)) return path;
  return BASE + path;
}

export async function initApi() {
  const res = await fetch(`${BASE}/api/token`);
  if (!res.ok) throw new Error('本地服务返回异常，请确认淘书工坊后端已启动');
  const data = await res.json();
  token = data.token;
  if (data.baseUrl) BASE = data.baseUrl;
  return data;
}

async function request(path, options = {}) {
  const { method = 'GET', body, blob, headers = {} } = options;
  const finalHeaders = { ...headers };
  if (token) finalHeaders['x-kf-token'] = token;
  const init = { method, headers: finalHeaders };
  if (blob) {
    init.body = blob;
    finalHeaders['content-type'] = blob.type || 'image/png';
  } else if (body !== undefined) {
    finalHeaders['content-type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  const res = await fetch(BASE + path, init);
  const text = await res.text();
  let data = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = text;
  }
  if (!res.ok) throw new Error((data && data.error) || `请求失败 HTTP ${res.status}`);
  return data;
}

export const api = {
  commerce: (path, body) =>
    request('/api/commerce' + path, body === undefined ? {} : { method: 'POST', body }),
  giteeConfig: () => request('/api/xianyu/gitee/config'),
  saveGiteeConfig: (body) => request('/api/xianyu/gitee/config', { method: 'PUT', body }),
  testGitee: () => request('/api/xianyu/gitee/test', { method: 'POST' }),
  xianyuConfig: () => request('/api/xianyu/config'),
  saveXianyuConfig: (body) => request('/api/xianyu/config', { method: 'PUT', body }),
  xianyuCall: (operation, body = {}) =>
    request('/api/xianyu/call/' + operation, { method: 'POST', body }),
  xianyuBookOrders: (id) => request('/api/xianyu/books/' + id + '/orders'),
  linkXianyuOrder: (orderNo, bookId) =>
    request('/api/xianyu/orders/' + encodeURIComponent(orderNo) + '/book', {
      method: 'POST',
      body: { bookId },
    }),
  xianyuTasks: () => request('/api/xianyu/tasks'),
  xianyuPreview: (id) => request('/api/xianyu/books/' + id + '/preview'),
  xianyuPublish: (id, body) =>
    request('/api/xianyu/books/' + id + '/publish', { method: 'POST', body }),
  xianyuTask: (id, action) => request('/api/xianyu/tasks/' + id + '/' + action, { method: 'POST' }),
  health: () => request('/api/health'),
  stats: () => request('/api/stats'),
  books: (params = {}) => {
    const query = new URLSearchParams();
    if (params.q) query.set('q', params.q);
    if (params.status) query.set('status', params.status);
    if (params.limit) query.set('limit', params.limit);
    const suffix = query.toString() ? `?${query}` : '';
    return request(`/api/books${suffix}`);
  },
  book: (id) => request(`/api/books/${id}`),
  createBook: (patch) => request('/api/books', { method: 'POST', body: patch }),
  updateBook: (id, patch) => request(`/api/books/${id}`, { method: 'PATCH', body: patch }),
  deleteBook: (id) => request(`/api/books/${id}`, { method: 'DELETE' }),

  importImages: (id, urls, source = 'kongfz') =>
    request(`/api/books/${id}/images`, { method: 'POST', body: { urls, source } }),
  uploadImage: (id, blob, source = 'upload') =>
    request(`/api/books/${id}/upload?source=${encodeURIComponent(source)}`, {
      method: 'POST',
      blob,
    }),
  saveProcessed: (imageId, blob) =>
    request(`/api/images/${imageId}/processed`, { method: 'POST', blob }),
  setPrimary: (imageId) => request(`/api/images/${imageId}/primary`, { method: 'POST' }),
  deleteImage: (imageId) => request(`/api/images/${imageId}`, { method: 'DELETE' }),
  reorderImages: (bookId, ids) =>
    request(`/api/books/${bookId}/images/order`, { method: 'PUT', body: { ids } }),

  setImportContext: (bookId) =>
    request('/api/import-context', { method: 'POST', body: { bookId } }),
  clearImportContext: () => request('/api/import-context', { method: 'DELETE' }),

  saveListing: (bookId, payload) =>
    request(`/api/books/${bookId}/listing`, { method: 'POST', body: payload }),
  listingPayload: (id) => request(`/api/listings/${id}/payload`),

  settings: () => request('/api/settings'),
  saveSettings: (patch) => request('/api/settings', { method: 'PUT', body: patch }),

  async exportCsv() {
    const res = await fetch(`${BASE}/api/export.csv`, {
      headers: token ? { 'x-kf-token': token } : {},
    });
    if (!res.ok) throw new Error('导出失败');
    return res.blob();
  },
};

const DEFAULT_URLS = {
  taobao: 'https://www.taobao.com/',
  kongfz: 'https://www.kongfz.com/',
  goofish: 'https://seller.goofish.com/?site=COMMONPRO#/seller-item/publish',
};

export async function openBrowser(kind, url) {
  const target = url || DEFAULT_URLS[kind];
  try {
    await invoke('open_browser', { kind, url: target });
  } catch (error) {
    console.warn('[open_browser] 回退到浏览器打开', error);
    window.open(target, '_blank', 'noopener');
  }
}

export function searchUrl(base, book) {
  const key = (book.isbn || book.title || '').trim();
  const root = base || 'https://search.kongfz.com/product_result/';
  const separator = root.includes('?') ? '&' : '?';
  return `${root}${separator}key=${encodeURIComponent(key)}`;
}
