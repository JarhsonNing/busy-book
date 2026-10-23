/* 淘书工坊 · Chrome 扩展后台
 * 负责访问本地 127.0.0.1 服务、获取工作台 token、代理 JSON 与图片请求。
 * 内容脚本运行在隔离世界，不接触页面自身的 JS 变量。
 */

const BASE = 'http://127.0.0.1:8899';
let token = null;

async function getToken(force = false) {
  if (token && !force) return token;
  const response = await fetch(`${BASE}/api/token`, { cache: 'no-store' });
  if (!response.ok) throw new Error(`本地服务返回 ${response.status}，请确认淘书工坊已启动`);
  const data = await response.json();
  token = data.token;
  setTimeout(() => {
    token = null;
  }, 4 * 60 * 1000);
  return token;
}

function blobToDataUrl(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(reader.error || new Error('图片读取失败'));
    reader.readAsDataURL(blob);
  });
}

async function handle(message) {
  const path = String(message.path || '');
  if (!path.startsWith('/api/')) throw new Error('非法接口路径');
  const method = message.method || 'GET';
  const raw = !!message.raw;

  const send = async () => {
    const headers = { accept: raw ? '*/*' : 'application/json' };
    if (path !== '/api/token' && path !== '/api/health') {
      headers['x-kf-token'] = await getToken();
    }
    let body;
    if (message.body !== undefined && message.body !== null) {
      headers['content-type'] = 'application/json';
      body = JSON.stringify(message.body);
    }
    return fetch(BASE + path, { method, headers, body, cache: 'no-store' });
  };

  let response = await send();
  if (response.status === 401 && path !== '/api/token') {
    token = null;
    response = await send();
  }

  if (raw) {
    if (!response.ok) {
      return { error: `图片请求失败：HTTP ${response.status}` };
    }
    const blob = await response.blob();
    return { dataUrl: await blobToDataUrl(blob), contentType: blob.type };
  }

  const text = await response.text();
  let data = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = { error: text };
  }
  if (!response.ok) {
    return { error: (data && data.error) || `HTTP ${response.status}` };
  }
  return { data };
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message || message.type !== 'taoshu:request') return false;
  handle(message)
    .then(sendResponse)
    .catch((error) => sendResponse({ error: error.message || String(error) }));
  return true;
});

chrome.runtime.onInstalled.addListener(() => {
  console.info('[淘书工坊] 扩展已安装。请确保桌面端已启动，并刷新淘宝/孔夫子/闲鱼页面。');
});
