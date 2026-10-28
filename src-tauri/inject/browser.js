/* 淘书工坊 · 内嵌浏览器注入脚本
 * 在淘宝/天猫、孔夫子、闲鱼卖家中心页面注入浮动工具条。
 * 通过本地 HTTP（或 Tauri IPC 兜底）与工作台通信。
 */
(function () {
  'use strict';
  if (window.top !== window) return;
  if (window.__TAOSHU_BOOTED__) return;
  window.__TAOSHU_BOOTED__ = true;

  let booted = false;

  function boot() {
  if (booted) return;
  booted = true;
  const CFG = window.__TAOSHU_CONFIG__ || {};
  const API = String(CFG.apiBase || 'http://127.0.0.1:8899').replace(/\/+$/, '');
  const SECRET = CFG.secret || '';
  const IS_TAURI = !!window.__TAURI_INTERNALS__;
  const IS_EXTENSION = !IS_TAURI && !!(window.chrome && window.chrome.runtime && window.chrome.runtime.id);
  const HOST = location.hostname;

  const SITE = (() => {
    if (/(^|\.)taobao\.com$/.test(HOST) || /(^|\.)tmall\.com$/.test(HOST)) return 'taobao';
    if (/(^|\.)kongfz\.com$/.test(HOST)) return 'kongfz';
    if (/(^|\.)goofish\.com$/.test(HOST) || /(^|\.)xianyu\.com$/.test(HOST)) return 'goofish';
    return 'other';
  })();

  const SITE_LABEL = { taobao: '淘宝', kongfz: '孔夫子', goofish: '闲鱼', other: '网页' }[SITE];

  const lookupParams = new URLSearchParams(location.search);
  let sourceBookId = SITE === 'taobao' && /^\d+$/.test(lookupParams.get('taoshuSourceBook') || '')
    ? Number(lookupParams.get('taoshuSourceBook')) : null;
  let lowestRequest = lookupParams.get('taoshuPriceSearch') || '';
  try {
    if (HOST === 's.taobao.com') {
      if (sourceBookId) sessionStorage.setItem('taoshu-source-search', JSON.stringify({bookId:sourceBookId,query:lookupParams.get('q'),createdAt:Date.now(),lowestRequest}));
      else {
        const previous = JSON.parse(sessionStorage.getItem('taoshu-source-search') || 'null');
        if (previous && Date.now()-previous.createdAt < 1800000 && previous.query === lookupParams.get('q')) {
          sourceBookId = previous.bookId;
          if (lookupParams.get('sort') === 'price-asc') lowestRequest = previous.lowestRequest || '';
        }
      }
    }
  } catch { /* Keep the explicit URL context when storage is unavailable. */ }
  const sourceSearch = !!sourceBookId && HOST === 's.taobao.com';
  function extractTaobaoSearchCandidates(limit = 20) {
    const items = new Map();
    for (const a of document.querySelectorAll('a[href*="item.taobao.com"],a[href*="detail.tmall.com"]')) {
      if (a.getClientRects && !a.getClientRects().length) continue;
      let url;
      try { url = new URL(a.href, location.href); } catch { continue; }
      if (!['item.taobao.com', 'detail.tmall.com'].includes(url.hostname) || !['http:', 'https:'].includes(url.protocol)) continue;
      const id = url.searchParams.get('id');
      if (!/^\d+$/.test(id || '') || items.has(id)) continue;
      const card = a.closest('[class*="doubleCardWrapper"],[class*="Card--"],.item,.m-item,li') || a;
      const titleNode = card.querySelector('[class*="title"],.title');
      const title = (titleNode?.textContent || a.getAttribute('title') || card.querySelector('img')?.alt || '').trim();
      if (!title) continue;
      const integer = card.querySelector('[class*="priceInt"]');
      const fraction = card.querySelector('[class*="priceFloat"]');
      const priceText = integer
        ? integer.textContent.trim() + (fraction?.textContent.trim() || '')
        : card.querySelector('[class*="innerPriceWrapper"],[class*="price"],.price')?.textContent || '';
      const price = priceText.replace(/,/g, '').match(/(?:[¥￥]\s*|^\s*)(\d+(?:\.\d+)?)(?![\d.])/)?.[1];
      const priceLabel = card.querySelector('[class*="priceDesc"]')?.textContent?.trim() || '';
      const shop = (card.querySelector('[class*="shopNameText"]') || card.querySelector('[class*="shopName"],.shopname,.shop'))?.textContent?.trim() || '';
      items.set(id, { asSource: true, sourceBookId, itemId: id,
        url: `https://${url.hostname}/item.htm?id=${id}`, title, shop,
        price: price ? Number(price) : null, priceLabel, images: [],
        raw: { source: 'taobao-search', query: lookupParams.get('q') || '', priceLabel, capturedAt: new Date().toISOString() } });
      if (items.size >= limit) break;
    }
    return [...items.values()];
  }
  function scrollTaobaoResults() {
    let root = document.querySelector('[class*="scrollWrapper"]');
    while (root && !(root.scrollHeight > root.clientHeight + 4 && /auto|scroll/.test(getComputedStyle(root).overflowY))) root = root.parentElement;
    root = root || document.scrollingElement || document.documentElement;
    const height = root.clientHeight || window.innerHeight;
    const atBottom = root.scrollTop + height >= root.scrollHeight - 4;
    root.scrollTo({top:root.scrollTop + Math.max(1, height * 0.85),behavior:'instant'});
    return atBottom;
  }
  function taobaoNextButton() {
    // The supplied HTML also has a simple carousel pager; only use the main results pager.
    return [...document.querySelectorAll('.next-pagination.next-normal button.next-next')]
      .filter(el => el.getClientRects().length && !el.disabled && el.getAttribute('aria-disabled') !== 'true').pop();
  }
  async function collectTaobaoSearch() {
    if (state.busy) return false;
    const items = extractTaobaoSearchCandidates();
    if (!items.length) return false;
    setBusy(true);
    let saved = 0;
    try {
      for (const item of items) {
        await requestJson('/api/captures', { method: 'POST', body: item });
        saved++;
      }
      toast(`已返回 ${saved} 条货源候选，请回到图书核对`, 'success');
      return true;
    } catch (e) {
      toast(`已保存 ${saved} 条，${e.message}；可重试补全`, 'error');
      return true;
    } finally { setBusy(false); refreshSiteButton(); }
  }
  async function runLowestPrice(retry = false) {
    if (state.busy) return;
    setBusy(true);
    const endpoint = '/api/commerce/books/' + sourceBookId + '/price-search';
    openPanel('一键找最低价', '正在读取淘宝搜索结果…');
    try {
      let task = await requestJson(endpoint);
      if (task.requestId !== lowestRequest) throw new Error('比价任务已更新，请从图书货源页重新打开');
      if (['completed','canceled'].includes(task.state)) { toast('比价已结束，返回图书查看结果', 'info'); return; }
      if (task.state === 'waiting') {
        if (!retry) { toast(task.error || '请完成网站登录或验证后点击继续比价', 'info'); return; }
        task = await requestJson(endpoint, {method:'POST',body:{action:'resume',requestId:lowestRequest}});
      }
      const known = new Set((task.items || []).map(i => i.url));
      const pending = new Map();
      let quiet = 0, awaitingNext = false, sawCards = false;
      const active = value => value.requestId === lowestRequest && value.state === 'searching';
      const flush = async () => {
        if (!pending.size) return;
        task = await requestJson(endpoint, {method:'POST',body:{action:'batch',requestId:lowestRequest,batch:(task.batchesDone ?? task.pagesDone) + 1,items:[...pending.values()]}});
        pending.clear();
        panelSub.textContent = `已采集 ${task.count} 个候选`;
      };
      for (let attempt = 0; attempt < 120; attempt++) {
        await new Promise(resolve => setTimeout(resolve, 1000));
        task = await requestJson(endpoint);
        if (!active(task)) return;
        const verification = [...document.querySelectorAll('iframe[src*="login.taobao.com"],iframe[src*="captcha"],iframe[src*="punish"],#baxia-dialog-content,.nc-container')]
          .some(el => el.getClientRects().length);
        if (verification) {
          await flush();
          if (!active(task)) break;
          throw new Error('请在淘宝窗口登录或完成安全验证后继续比价');
        }
        let added = 0;
        // Read all mounted cards: a fixed first-60 slice misses newly appended results.
        const visibleItems = extractTaobaoSearchCandidates(Infinity);
        if (visibleItems.length) sawCards = true;
        for (const item of visibleItems) {
          if (known.has(item.url)) continue;
          known.add(item.url); pending.set(item.url,item); added++;
          if (pending.size === 60) {
            await flush();
            if (!active(task)) break;
          }
        }
        if (!active(task)) break;
        if (added) { quiet = 0; awaitingNext = false; }
        const atBottom = scrollTaobaoResults();
        quiet = added || !atBottom ? 0 : quiet + 1;
        if (quiet < 6) continue;
        if (awaitingNext) throw new Error('下一批商品尚未加载，请检查淘宝页面后继续');
        await flush();
        if (!active(task)) break;
        const next = taobaoNextButton();
        if (next) {
          // Use the actual website control; never invent page offsets or reload to fetch a batch.
          task = await requestJson(endpoint);
          if (!active(task)) return;
          next.click(); quiet = 0; awaitingNext = true;
          continue;
        }
        if (!sawCards) throw new Error('未读取到搜索商品，请登录或完成页面验证后继续');
        task = await requestJson(endpoint, {method:'POST',body:{action:'finish',requestId:lowestRequest,reason:'visible-end'}});
        break;
      }
      if (active(task)) {
        await flush();
        if (active(task)) throw new Error('采集时间已到，结果已保留，可继续检索');
      }
      if (task.state === 'completed') {
        toast(`已采集 ${task.count} 个候选，请核对规格与运费`, 'success');
        await focusDashboard();
      } else if (task.state === 'waiting') { toast(task.error, 'info'); }
    } catch (e) {
      await requestJson(endpoint, {method:'POST',body:{action:'waiting',requestId:lowestRequest,error:e.message}}).catch(()=>{});
      panelSub.textContent = e.message;
      toast(e.message, 'error');
    } finally { setBusy(false); refreshSiteButton(); }
  }
  let bookLookup = SITE === 'kongfz' && lookupParams.get('taoshuLookup') ? {
    id: lookupParams.get('taoshuRequirement'), requestId: lookupParams.get('taoshuLookup'),
    query: lookupParams.get('keyword') || lookupParams.get('key') || '', createdAt: Date.now(),
  } : null;
  try {
    if (bookLookup) sessionStorage.setItem('taoshu-book-lookup', JSON.stringify(bookLookup));
    else if (HOST === 'search.kongfz.com') {
      const previous = JSON.parse(sessionStorage.getItem('taoshu-book-lookup') || 'null');
      if (previous && Date.now() - previous.createdAt < 600000 &&
          previous.query === (lookupParams.get('keyword') || lookupParams.get('key'))) bookLookup = previous;
    }
  } catch { /* Storage may be disabled; URL context still works. */ }
  async function searchBookMetadata() {
    if (!bookLookup || state.busy) return;
    setBusy(true);
    openPanel('检索图书资料', bookLookup.query);
    try {
      const params = new URLSearchParams({ keyword: bookLookup.query, page: '1', dataType: '1' });
      const response = await fetch('/pc-gw/search-web/client/pc/product/keyword/list?' + params, { credentials: 'same-origin', signal: AbortSignal.timeout(20000) });
      if (!response.ok) throw new Error('孔夫子返回 HTTP ' + response.status + '，请稍后重试');
      const result = await response.json();
      if (result.status !== 1) throw new Error(result.message || result.data?.requestRejectCause || '请在孔夫子登录或完成页面验证后，点击重试检索');
      if (!Array.isArray(result.data?.itemResponse?.list)) throw new Error('网站结果格式发生变化，请查看搜索页核对后重试');
      const items = result.data.itemResponse.list.map(b => {
        const title = String(b.bookName || b.itemName || b.title || '').replace(/<[^>]*>/g, '');
        return { title, author: b.author || '', publisher: b.press || '', isbn: b.isbn || '',
          edition: b.edition || title.match(/第[一二三四五六七八九十百\d]+版/)?.[0] || '',
          volume: b.volume || '', publicationYear: b.pubDate || '',
          sourceUrl: b.link?.pc || b.url || (b.shopId && b.itemId ? `https://book.kongfz.com/${b.shopId}/${b.itemId}/` : '') };
      });
      const saved = await requestJson('/api/commerce/requirements/' + encodeURIComponent(bookLookup.id) + '/search', {
        method: 'POST', body: { requestId: bookLookup.requestId, items },
      });
      toast(`已检索到 ${saved.items.length} 条候选，请返回匹配图书选择`, 'success');
      panelSub.textContent = `已返回 ${saved.items.length} 条候选`;
    } catch (e) {
      await requestJson('/api/commerce/requirements/' + encodeURIComponent(bookLookup.id) + '/search', {
        method: 'POST', body: { requestId: bookLookup.requestId, error: e.message },
      }).catch(() => {});
      toast(e.message, 'error');
      panelSub.textContent = e.message;
    } finally { setBusy(false); refreshSiteButton(); }
  }

  /* --------------------------- 新窗口跳转兼容 --------------------------- */
  /* WebView2 默认不弹出 target=_blank / window.open 新窗口，淘宝、孔夫子
     大量链接依赖新窗口，点击后会看起来“没有反应”。这里改成当前窗口跳转。 */
  function openInSameWindow(value) {
    if (!value) return false;
    try {
      const parsed = new URL(String(value), location.href);
      if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return false;
      if (sourceBookId && ['item.taobao.com','detail.tmall.com'].includes(parsed.hostname)) parsed.searchParams.set('taoshuSourceBook', String(sourceBookId));
      if (parsed.href === location.href) return false;
      location.assign(parsed.href);
      return true;
    } catch {
      return false;
    }
  }

  if (IS_TAURI && (SITE === 'taobao' || SITE === 'kongfz')) {
    document.addEventListener(
      'click',
      (event) => {
        const anchor = event.target && event.target.closest
          ? event.target.closest('a[href]')
          : null;
        if (!anchor) return;
        if (sourceBookId) {
          try {
            const url = new URL(anchor.href);
            if (['item.taobao.com','detail.tmall.com'].includes(url.hostname)) {
              url.searchParams.set('taoshuSourceBook', String(sourceBookId));
              anchor.href = url.href;
            }
          } catch { /* Ignore non-product links. */ }
        }
        const target = String(anchor.getAttribute('target') || '').trim().toLowerCase();
        if (!target || target === '_self') return;
        if (openInSameWindow(anchor.href)) event.preventDefault();
      },
      true
    );

    try {
      const nativeOpen = window.open;
      window.open = function (url, target, features) {
        if (openInSameWindow(url)) return window;
        return nativeOpen ? nativeOpen.call(window, url, target, features) : null;
      };
    } catch (error) {
      console.warn('[淘书工坊] 无法接管 window.open，将只处理 target=_blank 链接', error);
    }
  }

  /* ------------------------------- 通信层 ------------------------------- */

  async function httpJson(path, options) {
    const method = (options && options.method) || 'GET';
    const headers = { accept: 'application/json' };
    if (SECRET) headers['x-taoshu-secret'] = SECRET;
    let body;
    if (options && options.body !== undefined && options.body !== null) {
      headers['content-type'] = 'application/json';
      body = JSON.stringify(options.body);
    }
    const response = await fetch(API + path, {
      method,
      headers,
      body,
      credentials: 'omit',
      cache: 'no-store',
    });
    const text = await response.text();
    let data = null;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      data = { error: text };
    }
    if (!response.ok) throw new Error((data && data.error) || `HTTP ${response.status}`);
    return data;
  }

  async function bridgeCall(path, method, body) {
    const invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
    if (!invoke) throw new Error('本地桥接不可用');
    const result = await invoke('bridge_fetch', {
      secret: SECRET,
      path,
      method: method || 'GET',
      body: body === undefined ? null : body,
    });
    const binary = atob(result.bodyBase64 || '');
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return { status: result.status, contentType: result.contentType, bytes };
  }

  async function requestJson(path, options = {}) {
    if (IS_EXTENSION) {
      const result = await chrome.runtime.sendMessage({
        type: 'taoshu:request',
        path,
        method: options.method || 'GET',
        body: options.body === undefined ? null : options.body,
        raw: false,
      });
      if (!result || result.error) throw new Error((result && result.error) || '扩展通信失败');
      return result.data;
    }
    if (IS_TAURI) {
      try {
        const result = await bridgeCall(path, options.method || 'GET', options.body);
        const text = new TextDecoder().decode(result.bytes);
        let data = null;
        try {
          data = text ? JSON.parse(text) : null;
        } catch {
          data = { error: text };
        }
        if (result.status >= 400) throw new Error((data && data.error) || `HTTP ${result.status}`);
        return data;
      } catch (error) {
        console.warn('[淘书工坊] IPC 请求失败，尝试本地 HTTP', error);
      }
    }
    return await httpJson(path, options);
  }

  async function requestBlob(path) {
    if (IS_EXTENSION) {
      const result = await chrome.runtime.sendMessage({ type: 'taoshu:request', path, method: 'GET', raw: true });
      if (!result || result.error) throw new Error((result && result.error) || '扩展通信失败');
      const response = await fetch(result.dataUrl);
      return await response.blob();
    }
    if (IS_TAURI) {
      try {
        const result = await bridgeCall(path, 'GET');
        if (result.status >= 400) throw new Error(`HTTP ${result.status}`);
        return new Blob([result.bytes], { type: result.contentType || 'application/octet-stream' });
      } catch (error) {
        console.warn('[淘书工坊] IPC 取图失败，尝试本地 HTTP', error);
      }
    }
    const response = await fetch(API + path, { credentials: 'omit', cache: 'no-store' });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return await response.blob();
  }

  async function focusDashboard() {
    if (!IS_TAURI) return;
    try {
      await window.__TAURI_INTERNALS__.invoke('focus_main', { secret: SECRET });
    } catch (error) {
      console.warn('[淘书工坊] 聚焦主窗口失败', error);
    }
  }

  /* -------------------------------- 界面 -------------------------------- */

  const state = {
    collapsed: localStorage.getItem('taoshu.toolbar.collapsed') === '1',
    panelOpen: false,
    context: null,
    busy: false,
    task: null,
    address: location.href,
    online: false,
    lastPayload: null,
  };

  const host = document.createElement('div');
  host.id = '__taoshu_host__';
  host.style.cssText = 'all: initial; position: fixed; inset: 0; z-index: 2147483647; pointer-events: none;';
  document.documentElement.appendChild(host);
  const shadow = host.attachShadow({ mode: 'open' });
  shadow.innerHTML = `
    <style>
      *, *::before, *::after { box-sizing: border-box; }
      .ts { font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif; }
      .toasts { position: fixed; top: 18px; left: 50%; transform: translateX(-50%); display: flex; flex-direction: column; gap: 8px; align-items: center; pointer-events: none; }
      .ts-toast { pointer-events: auto; background: #241d16; color: #fdf2e2; padding: 10px 16px; border-radius: 10px; font-size: 13px; box-shadow: 0 18px 40px -18px rgba(0,0,0,.6); border: 1px solid #483a2b; max-width: 560px; line-height: 1.5; }
      .ts-toast.success { border-color: #3f7a5f; }
      .ts-toast.error { border-color: #a74834; }
      .ts-toast .link { color: #f0b35a; cursor: pointer; text-decoration: underline; margin-left: 6px; }
      .dock { position: fixed; left: 50%; bottom: 18px; transform: translateX(-50%); pointer-events: auto; width: max-content; max-width: calc(100vw - 24px); }
      .bar { display: flex; align-items: center; gap: 4px; background: rgba(32, 25, 18, .94); color: #f1e5d3; border: 1px solid #4a3b2b; border-radius: 13px; padding: 6px; box-shadow: 0 20px 50px -20px rgba(0,0,0,.65); backdrop-filter: blur(10px); }
      .bar button { border: 0; background: transparent; color: #e8dbc7; height: 30px; min-width: 30px; padding: 0 9px; border-radius: 8px; font-size: 12px; cursor: pointer; display: inline-flex; align-items: center; justify-content: center; gap: 5px; white-space: nowrap; }
      .bar button:hover { background: rgba(255, 245, 230, .12); color: #fff; }
      .bar button:disabled { opacity: .35; cursor: default; }
      .bar button.primary { background: #b4452f; color: #fff5ec; padding: 0 13px; }
      .bar button.primary:hover { background: #c4513a; }
      .bar button.accent { background: #2f6b52; color: #eafff3; padding: 0 13px; }
      .bar button.collapse { color: #a99a86; }
      .addr { width: 290px; height: 30px; border-radius: 8px; border: 1px solid #514231; background: #1a1510; color: #ddcfbb; font-size: 11.5px; padding: 0 10px; outline: none; }
      .addr:focus { border-color: #b4452f; }
      .sep { width: 1px; height: 18px; background: #4a3b2b; margin: 0 3px; }
      .brand-chip { display: inline-flex; align-items: center; gap: 6px; padding: 0 8px; font-size: 11px; color: #b8a893; }
      .brand-chip .dot { width: 6px; height: 6px; border-radius: 50%; background: #7a6a56; }
      .brand-chip .dot.on { background: #63b183; box-shadow: 0 0 0 3px rgba(99,177,131,.18); }
      .pill { display: inline-flex; align-items: center; gap: 7px; background: rgba(32, 25, 18, .94); color: #f1e5d3; border: 1px solid #4a3b2b; border-radius: 999px; padding: 7px 14px; font-size: 12px; cursor: pointer; box-shadow: 0 14px 30px -18px rgba(0,0,0,.6); }
      .pill:hover { border-color: #7a6449; }
      .panel { margin-bottom: 8px; background: rgba(32, 25, 18, .96); color: #ecdfcc; border: 1px solid #4a3b2b; border-radius: 14px; padding: 12px 14px; font-size: 12px; line-height: 1.6; max-height: 46vh; overflow: auto; box-shadow: 0 20px 50px -20px rgba(0,0,0,.65); display: none; }
      .panel.open { display: block; }
      .panel h4 { margin: 0 0 6px; font-size: 12px; color: #f5e8d5; display: flex; align-items: center; justify-content: space-between; gap: 10px; }
      .panel .muted { color: #9f8f7a; }
      .panel .log { margin: 6px 0 0; padding: 0; list-style: none; }
      .panel .log li { padding: 2px 0; color: #c8b8a3; }
      .panel .log li::before { content: "·"; margin-right: 6px; color: #8a7458; }
      .panel .log li.ok::before { content: "✓"; color: #6ab78b; }
      .panel .log li.warn::before { content: "!"; color: #e0b35a; }
      .panel .log li.err::before { content: "!"; color: #e07a60; }
      .panel .progress { height: 3px; border-radius: 999px; background: #3d3023; overflow: hidden; margin-top: 8px; }
      .panel .progress > i { display: block; height: 100%; width: 30%; background: #b4452f; animation: slide 1.2s ease-in-out infinite; }
      .panel .copy-row { display: flex; align-items: flex-start; gap: 8px; padding: 6px 0; border-top: 1px dashed #4a3b2b; }
      .panel .copy-row:first-of-type { border-top: 0; }
      .panel .copy-row .k { flex: 0 0 46px; color: #9f8f7a; }
      .panel .copy-row .v { flex: 1; min-width: 0; color: #e8dbc7; word-break: break-word; max-height: 110px; overflow: auto; white-space: pre-wrap; }
      .panel button { border: 1px solid #5b4936; background: #2c241b; color: #ecdfcc; height: 28px; padding: 0 10px; border-radius: 7px; font-size: 12px; cursor: pointer; }
      .panel button:hover { border-color: #8a6d4e; color: #fff; }
      .panel button.primary { background: #b4452f; border-color: #b4452f; color: #fff5ec; }
      .panel .panel-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 10px; }
      @keyframes slide { 0% { margin-left: -30%; } 100% { margin-left: 100%; } }
      .picker { position: fixed; inset: 0; background: rgba(20, 15, 10, .72); backdrop-filter: blur(4px); display: flex; align-items: center; justify-content: center; padding: 24px; pointer-events: auto; }
      .picker-box { width: min(980px, 100%); max-height: 88vh; background: #fffdf8; color: #241d16; border-radius: 18px; border: 1px solid #e2d7c4; display: flex; flex-direction: column; overflow: hidden; box-shadow: 0 40px 90px -40px rgba(0,0,0,.7); }
      .picker-head { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 15px 18px; border-bottom: 1px solid #ece2d1; }
      .picker-head h3 { margin: 0; font-size: 16px; }
      .picker-head p { margin: 3px 0 0; font-size: 11px; color: #8c8275; }
      .picker-head button { border: 0; background: #f2ece1; width: 30px; height: 30px; border-radius: 9px; font-size: 16px; cursor: pointer; }
      .picker-grid { padding: 14px 18px; overflow: auto; display: grid; grid-template-columns: repeat(auto-fill, minmax(118px, 1fr)); gap: 10px; background: #faf6ee; }
      .pick { position: relative; aspect-ratio: 3 / 4; border-radius: 10px; overflow: hidden; border: 2px solid transparent; background: #ebe3d6; cursor: pointer; }
      .pick img { width: 100%; height: 100%; object-fit: cover; display: block; }
      .pick input { position: absolute; opacity: 0; pointer-events: none; }
      .pick .tick { position: absolute; top: 6px; right: 6px; width: 20px; height: 20px; border-radius: 50%; border: 2px solid #fff; background: rgba(0,0,0,.35); display: grid; place-items: center; color: transparent; font-size: 12px; }
      .pick.selected { border-color: #b4452f; }
      .pick.selected .tick { background: #b4452f; color: #fff; }
      .picker-foot { display: flex; align-items: center; gap: 10px; padding: 12px 18px; border-top: 1px solid #ece2d1; }
      .picker-foot .spacer { flex: 1; }
      .picker-foot button { height: 34px; padding: 0 14px; border-radius: 9px; border: 1px solid #d8cbb8; background: #fff; font-size: 12px; cursor: pointer; }
      .picker-foot button.primary { background: #b4452f; border-color: #b4452f; color: #fff7ef; }
      .picker-foot button:disabled { opacity: .5; cursor: default; }
      .picker-foot .count { font-size: 12px; color: #8c8275; }
      .empty-hint { padding: 50px; text-align: center; color: #8c8275; font-size: 13px; grid-column: 1 / -1; }
      .hidden { display: none !important; }
      @media (max-width: 900px) { .addr { width: 160px; } .brand-chip { display: none; } }
    </style>
    <div class="ts">
      <div class="toasts"></div>
      <div class="dock">
        <div class="panel" data-panel>
          <h4><span data-panel-title>淘书助手</span><span class="muted" data-panel-sub></span></h4>
          <div data-panel-body></div>
        </div>
        <div class="bar" data-bar>
          <span class="brand-chip"><span class="dot" data-online></span><span data-site>${SITE_LABEL}</span></span>
          <button data-act="back" title="后退">←</button>
          <button data-act="forward" title="前进">→</button>
          <button data-act="reload" title="刷新">⟳</button>
          <input class="addr" data-addr spellcheck="false" />
          <button data-act="go">打开</button>
          <span class="sep"></span>
          <button class="primary" data-act="site" data-site-action>加载中…</button>
          <button data-act="dashboard" title="回到淘书工坊">工作台</button>
          <button class="collapse" data-act="collapse" title="收起">▾</button>
        </div>
        <div class="pill hidden" data-pill>⌘ 淘书助手</div>
      </div>
      <div data-modal></div>
    </div>
  `;

  const $ = (selector) => shadow.querySelector(selector);
  const addrInput = $('[data-addr]');
  const panel = $('[data-panel]');
  const panelTitle = $('[data-panel-title]');
  const panelSub = $('[data-panel-sub]');
  const panelBody = $('[data-panel-body]');
  const bar = $('[data-bar]');
  const pill = $('[data-pill]');
  const siteButton = $('[data-site-action]');
  const onlineDot = $('[data-online]');

  addrInput.value = location.href;

  shadow.addEventListener('click', (event) => {
    const copyTarget = event.target.closest('[data-copy]');
    if (copyTarget) {
      copyManual(copyTarget.getAttribute('data-copy'));
      return;
    }
    const target = event.target.closest('[data-act]');
    if (!target) return;
    const action = target.getAttribute('data-act');
    if (action === 'back') history.back();
    if (action === 'forward') history.forward();
    if (action === 'reload') location.reload();
    if (action === 'go') navigate(addrInput.value);
    if (action === 'dashboard') {
      focusDashboard();
      toast(IS_TAURI ? '已切换到工作台窗口' : '当前为浏览器模式，请回到工作台标签页');
    }
    if (action === 'collapse') setCollapsed(true);
    if (action === 'site') runSiteAction();
  });

  pill.addEventListener('click', () => setCollapsed(false));

  addrInput.addEventListener('keydown', (event) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      navigate(addrInput.value);
    }
  });

  function navigate(value) {
    let url = String(value || '').trim();
    if (!url) return;
    if (!/^https?:\/\//i.test(url)) {
      if (/^[\w-]+(\.[\w-]+)+/.test(url)) url = 'https://' + url;
      else url = 'https://www.baidu.com/s?wd=' + encodeURIComponent(url);
    }
    location.href = url;
  }

  function setCollapsed(collapsed) {
    state.collapsed = collapsed;
    localStorage.setItem('taoshu.toolbar.collapsed', collapsed ? '1' : '0');
    bar.classList.toggle('hidden', collapsed);
    pill.classList.toggle('hidden', !collapsed);
    if (collapsed) panel.classList.remove('open');
  }

  function openPanel(title, subtitle) {
    if (title) panelTitle.textContent = title;
    panelSub.textContent = subtitle || '';
    panel.classList.add('open');
    state.panelOpen = true;
  }

  function setPanelBody(html) {
    panelBody.innerHTML = html;
  }

  function appendLog(text, kind) {
    let list = panelBody.querySelector('.log');
    if (!list) {
      list = document.createElement('ul');
      list.className = 'log';
      panelBody.appendChild(list);
    }
    const item = document.createElement('li');
    if (kind) item.className = kind;
    item.textContent = text;
    list.appendChild(item);
    panel.scrollTop = panel.scrollHeight;
  }

  function toast(message, type = 'info', ttl = 4200) {
    const node = document.createElement('div');
    node.className = 'ts-toast ' + type;
    node.textContent = message;
    shadow.querySelector('.toasts').appendChild(node);
    setTimeout(() => node.remove(), ttl);
    return node;
  }

  function fallbackCopy(value, done) {
    try {
      const area = document.createElement('textarea');
      area.value = value;
      area.style.cssText = 'position:fixed;left:-9999px;top:0;opacity:0';
      document.body.appendChild(area);
      area.select();
      const ok = document.execCommand('copy');
      area.remove();
      if (ok) done();
      else toast('复制失败，请手动选中文本复制', 'error');
    } catch {
      toast('复制失败，请手动选中文本复制', 'error');
    }
  }

  function copyText(value) {
    const text = String(value || '');
    if (!text) {
      toast('没有可复制的内容', 'error');
      return;
    }
    const done = () => toast('已复制到剪贴板', 'success', 1600);
    if (navigator.clipboard && window.isSecureContext) {
      navigator.clipboard.writeText(text).then(done).catch(() => fallbackCopy(text, done));
    } else {
      fallbackCopy(text, done);
    }
  }

  function copyManual(kind) {
    const payload = state.lastPayload;
    if (!payload) {
      toast('没有可复制的内容', 'error');
      return;
    }
    const map = {
      title: payload.title || '',
      price: String(payload.price ?? ''),
      desc: payload.description || '',
      all: `${payload.title || ''}\n\n价格：¥${payload.price}\n\n${payload.description || ''}`,
    };
    copyText(map[kind] ?? '');
  }

  function showManualPanel(payload, reason) {
    state.lastPayload = payload;
    const imageNote =
      payload.images && payload.images.length
        ? `图片 ${payload.images.length} 张已存入本地书库，请在闲鱼发布页手动选择本地图片上传。`
        : '当前没有图片，建议先回工作台补图。';
    setPanelBody(`
      <p class="muted">${escapeHtml(reason || '自动填表没有完成，请手动复制内容到闲鱼发布页。')}</p>
      <div class="copy-row"><span class="k">标题</span><span class="v">${escapeHtml(payload.title || '')}</span><button data-copy="title">复制</button></div>
      <div class="copy-row"><span class="k">价格</span><span class="v">¥${escapeHtml(payload.price)}</span><button data-copy="price">复制</button></div>
      <div class="copy-row"><span class="k">描述</span><span class="v">${escapeHtml(payload.description || '')}</span><button data-copy="desc">复制</button></div>
      <p class="muted">${escapeHtml(imageNote)}</p>
      <div class="panel-actions">
        <button class="primary" data-act="site">再试一次自动填表</button>
        <button data-copy="all">复制全部文案</button>
      </div>
    `);
    openPanel('闲鱼发布任务', '手动复制模式');
    toast('自动填表未完成，已切换为手动复制模式', 'error', 7000);
  }

  function setBusy(flag) {
    state.busy = flag;
    siteButton.disabled = flag;
  }

  /* ------------------------------ 通用小工具 ----------------------------- */

  function visible(element) {
    if (!element) return false;
    const rect = element.getBoundingClientRect();
    if (rect.width < 2 || rect.height < 2) return false;
    const style = getComputedStyle(element);
    return style.display !== 'none' && style.visibility !== 'hidden' && Number(style.opacity) > 0.05;
  }

  function cleanText(value) {
    return String(value || '').replace(/\s+/g, ' ').trim();
  }

  function normalizeUrl(value) {
    if (!value) return '';
    let url = String(value).trim();
    if (url.startsWith('//')) url = 'https:' + url;
    if (url.startsWith('data:') || url.startsWith('blob:')) return '';
    try {
      const parsed = new URL(url, location.href);
      if (!/^https?:$/.test(parsed.protocol)) return '';
      return parsed.href;
    } catch {
      return '';
    }
  }

  function moneyNumber(value) {
    if (value === null || value === undefined) return null;
    const text = String(value).replace(/,/g, '');
    const match = text.match(/\d+(?:\.\d{1,2})?/);
    if (!match) return null;
    const number = Number(match[0]);
    return Number.isFinite(number) ? number : null;
  }

  function firstText(selectors) {
    for (const selector of selectors) {
      let nodes;
      try {
        nodes = document.querySelectorAll(selector);
      } catch {
        continue;
      }
      for (const node of nodes) {
        const text = cleanText(node.innerText || node.textContent);
        if (text && text.length <= 160) return text;
      }
    }
    return '';
  }

  function pageText() {
    return cleanText(document.body ? document.body.innerText : '');
  }

  function setNativeValue(element, value) {
    const prototype =
      element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const descriptor = Object.getOwnPropertyDescriptor(prototype, 'value');
    if (descriptor && descriptor.set) descriptor.set.call(element, value);
    else element.value = value;
    element.dispatchEvent(new Event('input', { bubbles: true }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
    element.dispatchEvent(new Event('blur', { bubbles: true }));
  }

  /* ------------------------------ 页面识别 ------------------------------ */

  function taobaoItemId() {
    const match =
      location.search.match(/[?&](?:id|itemId|item_id)=(\d+)/i) ||
      location.pathname.match(/\/i(\d+)\.htm/);
    return match ? match[1] : '';
  }

  function upgradeTaobaoImage(url) {
    return url
      .replace(/_\d+x\d+(?:q\d+)?\.(jpg|jpeg|png|webp)(\?.*)?$/i, '.$1')
      .replace(/_q\d+(?:\.(jpg|jpeg|png|webp))(\?.*)?$/i, '$1')
      .replace(/_sum\.(jpg|jpeg|png|webp)(\?.*)?$/i, '.$1')
      .replace(/_\.webp(\?.*)?$/i, '');
  }

  function upgradeGenericImage(url) {
    let result = url
      .replace(/([?&])x-oss-process=[^&]*/i, '$1')
      .replace(/_(\d{2,4})x(\d{2,4})(\.(?:jpe?g|png|webp))(?:\?.*)?$/i, '$3')
      .replace(/!(\d+|[a-z]+)(\.(?:jpe?g|png|webp))(?:\?.*)?$/i, '$2')
      .replace(/[?&]$/, '');
    // 孔夫子缩略图 _s.jpg 有小图/大图两套，换成 _b.jpg 拿清晰封面。
    try {
      const parsed = new URL(result);
      if (/(^|\.)(?:kfzimg\.com|kongfz\.com)$/i.test(parsed.hostname)) {
        result = result.replace(/_s(\.(?:jpe?g|png|webp))(\?.*)?$/i, '_b$1$2');
      }
    } catch {
      /* 忽略非法 URL */
    }
    return result;
  }

  function isJunkImage(url, element) {
    const lower = url.toLowerCase();
    if (/(logo|icon|avatar|qrcode|qr_code|code-mp|code-app|erweima|blank|spacer|loading|placeholder|sprite|badge)/.test(lower)) return true;
    try {
      const parsed = new URL(lower);
      // 孔夫子统计像素 / 用页面地址冒充图片的占位节点
      if (/(^|\.)tj\./.test(parsed.hostname) || /\/collect\//.test(parsed.pathname)) return true;
      if (/\/1\.gif$/.test(parsed.pathname)) return true;
      const hasImageExt = /\.(?:jpe?g|png|gif|webp|bmp|avif)$/.test(parsed.pathname);
      if (!hasImageExt && /(^|\.)kongfz\.com$/i.test(parsed.hostname)) return true;
    } catch {
      return true;
    }
    const width = element && (element.naturalWidth || 0);
    const height = element && (element.naturalHeight || 0);
    if (width && height && width <= 1 && height <= 1) return true;
    if (width && width < 100) return true;
    if (height && height < 100) return true;
    const alt = cleanText(element && element.getAttribute('alt')).toLowerCase();
    if (/logo|图标|头像|二维码/.test(alt)) return true;
    return false;
  }

  function collectPageImages(minSize) {
    const limit = minSize || 140;
    const found = new Map();

    const consider = (raw, element) => {
      let url = normalizeUrl(raw);
      if (!url) return;
      if (isJunkImage(url, element)) return;
      url = SITE === 'taobao' ? upgradeTaobaoImage(url) : upgradeGenericImage(url);
      const key = url.replace(/\?.*$/, '').replace(/_\d+x\d+/, '');
      if (found.has(key)) return;
      const width = (element && element.naturalWidth) || 0;
      const height = (element && element.naturalHeight) || 0;
      if (width && width < limit) return;
      found.set(key, { url, width, height, element });
    };

    document.querySelectorAll('img').forEach((image) => {
      consider(image.currentSrc || image.src, image);
      for (const attribute of ['data-src', 'data-original', 'data-lazy-src', 'data-ks-lazyload', 'data-url']) {
        consider(image.getAttribute(attribute), image);
      }
      const srcset = image.getAttribute('srcset') || image.getAttribute('data-srcset');
      if (srcset) {
        srcset.split(',').forEach((part) => consider(part.trim().split(/\s+/)[0], image));
      }
    });

    document.querySelectorAll('*').forEach((element) => {
      if (found.size >= 60) return;
      const style = element.getAttribute('style') || '';
      if (!style.includes('background')) return;
      const rect = element.getBoundingClientRect();
      if (rect.width < 120 || rect.height < 120) return;
      const match = style.match(/url\(["']?([^"')]+)["']?\)/i);
      if (match) consider(match[1], null);
    });

    return [...found.values()]
      .sort((a, b) => (b.width || 0) * (b.height || 0) - (a.width || 0) * (a.height || 0))
      .slice(0, 60);
  }

  /* -------------------------------- 淘宝 -------------------------------- */

  function extractTaobao() {
    const itemId = taobaoItemId();
    const bodyText = pageText();

    const title =
      firstText([
        '[class*="mainTitle"]',
        '[class*="MainTitle"]',
        '[class*="ItemTitle"]',
        '[class*="itemTitle"]',
        '.tb-main-title',
        '#J_Title h3',
        'h1',
        'meta[property="og:title"]',
      ]) ||
      cleanText(document.title).replace(/[-_].*(淘宝|天猫).*$/, '').trim();

    let price = null;
    const metaPrice = document.querySelector('meta[property="product:price:amount"]');
    if (metaPrice) price = moneyNumber(metaPrice.getAttribute('content'));
    if (price === null) {
      const priceText = firstText([
        '[class*="priceText"]',
        '[class*="Price--priceText"]',
        '[class*="highlightPrice"]',
        '[class*="priceWrap"] [class*="price"]',
        '#J_StrPrice .tb-rmb-num',
        '#J_PromoPriceNum',
        '.tm-price',
      ]);
      price = moneyNumber(priceText);
    }
    if (price === null) {
      const match = bodyText.match(/[¥￥]\s*(\d+(?:\.\d{1,2})?)/);
      if (match) price = Number(match[1]);
    }

    let originalPrice = null;
    const originalText = firstText([
      '[class*="originPrice"]',
      '[class*="originalPrice"]',
      '[class*="priceOrigin"]',
      '.tb-price',
      'del',
    ]);
    originalPrice = moneyNumber(originalText);

    const shop = firstText([
      '[class*="ShopHeader"] [class*="title"]',
      '[class*="shopName"]',
      '[class*="ShopName"]',
      '.shop-name',
      '.tb-shop-name',
      '#shopExtra .shop-name',
    ]);

    const salesMatch = bodyText.match(/(\d[\d.]*万?)\s*(人付款|人已买|人收货|人想要|月销)/);
    const sales = salesMatch ? `${salesMatch[1]}${salesMatch[2]}` : '';

    const images = [];
    const pushImage = (raw) => {
      let url = normalizeUrl(raw);
      if (!url) return;
      if (url.includes('alicdn.com')) url = upgradeTaobaoImage(url);
      if (isJunkImage(url, null)) return;
      if (!images.includes(url)) images.push(url);
    };

    document.querySelectorAll('#J_ImgBooth, [class*="mainPic"] img, [class*="PicGallery"] img, #J_UlThumb img, [class*="thumbnails"] img').forEach((image) => {
      pushImage(image.currentSrc || image.src);
      pushImage(image.getAttribute('data-src'));
    });

    const ogImage = document.querySelector('meta[property="og:image"]');
    if (ogImage) pushImage(ogImage.getAttribute('content'));

    collectPageImages(300).forEach((entry) => pushImage(entry.url));

    const specs = {};
    document.querySelectorAll('.tm-table tr, [class*="attributes"] li, [class*="param"] li, [class*="infoItem"]').forEach((row) => {
      const text = cleanText(row.innerText);
      const match = text.match(/^([^：:]{1,14})[：:]\s*(.+)$/);
      if (match) specs[match[1].trim()] = match[2].trim().slice(0, 120);
    });
    const isbnMatch = bodyText.match(/ISBN[^0-9]{0,4}([0-9][0-9-]{9,16})/i);
    if (isbnMatch) specs.isbn = isbnMatch[1].replace(/-/g, '');

    return {
      itemId,
      url: location.href,
      title,
      price,
      originalPrice,
      shop,
      sales,
      mainImage: images[0] || '',
      images: images.slice(0, 12),
      specs,
      raw: {
        pageTitle: document.title,
        capturedAt: new Date().toISOString(),
        source: 'embedded-browser',
      },
    };
  }

  async function captureTaobao() {
    if (state.busy) return;
    setBusy(true);
    openPanel('抓取淘宝页面', '正在读取商品信息…');
    setPanelBody('<div class="progress"><i></i></div>');
    try {
      const payload = extractTaobao();
      payload.asSource = true;
      if (sourceBookId) payload.sourceBookId = sourceBookId;
      if (!payload.title) throw new Error('没有识别到商品标题，请确认当前是商品详情页');
      if (!payload.itemId) throw new Error('没有识别到商品 ID，请打开具体商品页');

      appendLog(`商品：${payload.title.slice(0, 40)}`);
      appendLog(`价格：${payload.price ?? '未识别'} 元`);
      appendLog(`图片：${payload.images.length} 张`);

      const result = await requestJson('/api/captures', { method: 'POST', body: payload });
      const book = result.book;
      appendLog(sourceBookId ? '已返回本书货源页，请核对规格、价格及代发' : '已保存货源候选，请回到图书的货源页选择并核对', 'ok');
      toast(`货源已采集：${payload.title.slice(0, 24)}`, 'success');
      panelSub.textContent = '入库成功';
      if (book) {
        setPanelBody(
          `<ul class="log"><li class="ok">书目 #${book.id} · ${book.title}</li><li>成本 ¥${book.costPrice ?? '-'} · 建议售价 ¥${book.sellPrice ?? '-'}</li></ul>` +
            '<p class="muted">回到工作台即可找图、处理并发布。</p>'
        );
      }
    } catch (error) {
      appendLog(error.message || '抓取失败', 'err');
      toast(error.message || '抓取失败', 'error');
      panelSub.textContent = '抓取失败';
    } finally {
      setBusy(false);
      refreshSiteButton();
    }
  }

  /* ------------------------------- 孔夫子 ------------------------------- */

  async function loadImportContext() {
    try {
      const context = await requestJson('/api/import-context');
      state.context = context && context.active ? context : null;
    } catch {
      state.context = null;
    }
    refreshSiteButton();
    return state.context;
  }

  function openImagePicker() {
    const context = state.context;
    if (!context) {
      toast('先回到工作台，在书目上点「找图」', 'error');
      return;
    }
    const images = collectPageImages(120);
    const modal = $('[data-modal]');

    const box = document.createElement('div');
    box.className = 'picker';
    box.innerHTML = `
      <div class="picker-box">
        <div class="picker-head">
          <div>
            <h3>导入到《${escapeHtml(context.title || '未命名图书')}》</h3>
            <p>本页识别到 ${images.length} 张图片，选中后下载到本地书库</p>
          </div>
          <button data-close>×</button>
        </div>
        <div class="picker-grid">
          ${
            images.length
              ? images
                  .map(
                    (image, index) => `
            <label class="pick selected" data-index="${index}">
              <img src="${escapeHtml(image.url)}" loading="lazy" alt="" />
              <input type="checkbox" checked />
              <span class="tick">✓</span>
            </label>`
                  )
                  .join('')
              : '<div class="empty-hint">没有找到可用图片，请打开具体商品页（详情页）再试。</div>'
          }
        </div>
        <div class="picker-foot">
          <button data-all>全选 / 反选</button>
          <span class="count" data-count>已选 ${images.length} 张</span>
          <span class="spacer"></span>
          <button data-close>取消</button>
          <button class="primary" data-import ${images.length ? '' : 'disabled'}>导入选中图片</button>
        </div>
      </div>`;

    modal.appendChild(box);

    const updateCount = () => {
      const selected = box.querySelectorAll('.pick.selected').length;
      const label = box.querySelector('[data-count]');
      if (label) label.textContent = `已选 ${selected} 张`;
      const button = box.querySelector('[data-import]');
      if (button) button.disabled = selected === 0;
    };

    box.addEventListener('change', (event) => {
      const pick = event.target.closest('.pick');
      if (!pick) return;
      pick.classList.toggle('selected', event.target.checked);
      updateCount();
    });

    box.addEventListener('click', async (event) => {
      if (event.target.closest('[data-close]')) {
        box.remove();
        return;
      }
      if (event.target.closest('[data-all]')) {
        const picks = [...box.querySelectorAll('.pick')];
        const allSelected = picks.every((pick) => pick.classList.contains('selected'));
        picks.forEach((pick) => {
          pick.classList.toggle('selected', !allSelected);
          const input = pick.querySelector('input');
          if (input) input.checked = !allSelected;
        });
        updateCount();
        return;
      }
      const importButton = event.target.closest('[data-import]');
      if (!importButton) return;
      const urls = [...box.querySelectorAll('.pick.selected')].map(
        (pick) => images[Number(pick.getAttribute('data-index'))].url
      );
      if (!urls.length) return;
      importButton.disabled = true;
      importButton.textContent = '正在下载…';
      try {
        const result = await requestJson(`/api/books/${context.bookId}/images`, {
          method: 'POST',
          body: { urls, source: 'kongfz' },
        });
        const imported = result.imported ? result.imported.length : 0;
        const errors = result.errors ? result.errors.length : 0;
        box.remove();
        openPanel('找图完成', `已导入 ${imported} 张`);
        setPanelBody(
          `<ul class="log"><li class="ok">成功 ${imported} 张${errors ? `，失败 ${errors} 张` : ''}</li></ul>` +
            '<p class="muted">回到工作台即可看到图片，点图片上的「净化」处理水印。</p>'
        );
        toast(`已导入 ${imported} 张图片${errors ? `，${errors} 张失败` : ''}`, errors ? 'error' : 'success');
        if (errors) console.warn('[淘书工坊] 导入失败', result.errors);
      } catch (error) {
        importButton.disabled = false;
        importButton.textContent = '导入选中图片';
        toast(error.message || '导入失败', 'error');
      }
    });

    // 尝试加载更大尺寸的原图
    images.slice(0, 24).forEach((image, index) => {
      const candidate = upgradeGenericImage(image.url);
      if (candidate === image.url) return;
      const probe = new Image();
      probe.onload = () => {
        if (probe.naturalWidth > (image.width || 0) + 20) {
          images[index].url = candidate;
          const pick = box.querySelector(`.pick[data-index="${index}"] img`);
          if (pick) pick.src = candidate;
        }
      };
      probe.src = candidate;
    });
  }

  function escapeHtml(value) {
    return String(value || '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  async function runKongfzAction() {
    const context = await loadImportContext();
    if (!context) {
      toast('先从工作台点「找图」，再在这里选图', 'error');
      return;
    }
    openImagePicker();
  }

  /* -------------------------------- 闲鱼 -------------------------------- */

  const TASK_KEY = 'taoshu.publish_task';

  function readTaskFromUrl() {
    try {
      const url = new URL(location.href);
      let id = url.searchParams.get('kf_listing');
      if (!id && url.hash) {
        const match = url.hash.match(/kf_listing=(\d+)/);
        if (match) id = match[1];
      }
      if (!id) return null;
      url.searchParams.delete('kf_listing');
      if (url.hash.includes('kf_listing')) url.hash = '';
      history.replaceState(history.state, '', url.toString());
      return { id: Number(id), ts: Date.now() };
    } catch {
      return null;
    }
  }

  function loadTask() {
    const fromUrl = readTaskFromUrl();
    if (fromUrl) {
      sessionStorage.setItem(TASK_KEY, JSON.stringify(fromUrl));
      return fromUrl;
    }
    try {
      const raw = sessionStorage.getItem(TASK_KEY);
      if (!raw) return null;
      const task = JSON.parse(raw);
      if (!task || !task.id) return null;
      if (Date.now() - (task.ts || 0) > 2 * 60 * 60 * 1000) return null;
      return task;
    } catch {
      return null;
    }
  }

  function clearTask() {
    sessionStorage.removeItem(TASK_KEY);
    state.task = null;
  }

  function findPublishEntry() {
    const pattern = /^(发布|卖闲置|发布闲置|发布宝贝|发闲置|我要卖|出售)$/;
    const candidates = [
      ...document.querySelectorAll(
        'a, button, [role="button"], [class*="btn"], [class*="menu"] a, [class*="menu"] div, [class*="publish"], [class*="sell"]'
      ),
    ].filter(visible);
    for (const element of candidates) {
      const text = cleanText(element.innerText || element.textContent);
      if (text && text.length <= 10 && pattern.test(text)) return element;
    }
    const byHref = document.querySelector('a[href*="publish"], a[href*="sell"], a[href*="item/publish"]');
    return visible(byHref) ? byHref : null;
  }

  function findAllInputs() {
    return [...document.querySelectorAll('input, textarea, [contenteditable="true"]')].filter(visible);
  }

  function hintText(element) {
    return cleanText(
      [
        element.getAttribute('placeholder'),
        element.getAttribute('aria-label'),
        element.getAttribute('name'),
        element.getAttribute('id'),
        element.getAttribute('class'),
        element.getAttribute('data-testid'),
      ]
        .filter(Boolean)
        .join(' ')
    ).toLowerCase();
  }

  function findInput(tags, hints, excludes = []) {
    const inputs = findAllInputs().filter((element) => tags.includes(element.tagName.toLowerCase()));
    for (const element of inputs) {
      const hint = hintText(element);
      if (excludes.some((word) => hint.includes(word))) continue;
      if (hint && hints.some((word) => hint.includes(word))) return element;
    }
    return inputs.length === 1 ? inputs[0] : null;
  }

  function labelText(label) {
    return cleanText(label ? label.innerText || label.textContent : '')
      .replace(/\s*\*+\s*$/, '')
      .trim();
  }

  function findFormItem(labelName) {
    const labels = [...document.querySelectorAll('label')].filter(visible);
    for (const label of labels) {
      const text = labelText(label);
      if (!text) continue;
      const matched = text === labelName || (labelName.length > 2 && text.startsWith(labelName));
      if (!matched) continue;
      // 不能用 [class*="form-item"]：label 自身可能带 ant-form-item-required，
      // closest 会命中 label 而不是外层表单项，导致找不到输入框。
      const item = label.closest('.ant-form-item, .form-group, tr');
      if (item) return { item, label };
      let node = label.parentElement;
      for (let depth = 0; node && depth < 4; depth += 1, node = node.parentElement) {
        if (node.querySelector('input:not([type="file"]):not([type="hidden"]), textarea, [contenteditable="true"]')) {
          return { item: node, label };
        }
      }
    }
    return null;
  }

  function controlInItem(item, selector) {
    if (!item) return null;
    const query = selector || 'input:not([type="file"]):not([type="hidden"]), textarea, [contenteditable="true"]';
    return [...item.querySelectorAll(query)].find(visible) || null;
  }

  function findLabeledControl(labelName, selector) {
    const found = findFormItem(labelName);
    if (!found) return null;
    const forId = found.label.getAttribute('for');
    if (forId) {
      const target = document.getElementById(forId);
      if (target) {
        if (target.matches('input, textarea, [contenteditable="true"]') && visible(target)) return target;
        const nested = controlInItem(target, selector);
        if (nested) return nested;
      }
    }
    return controlInItem(found.item, selector);
  }

  function findPublishForm() {
    const inputSelector = 'input:not([type="file"]):not([type="hidden"])';
    const title =
      findLabeledControl('宝贝标题', inputSelector) ||
      findLabeledControl('标题', inputSelector) ||
      findInput(['input'], ['标题', 'title', '名称', '宝贝'], ['搜索', 'search']);
    const price =
      findLabeledControl('价格', inputSelector) ||
      findInput(['input'], ['价格', '售价', 'price', '金额'], ['原价', '邮费', '运费', 'search']);
    const description =
      findLabeledControl('宝贝描述', 'textarea, [contenteditable="true"]') ||
      findLabeledControl('描述', 'textarea, [contenteditable="true"]') ||
      findInput(['textarea'], ['描述', '详情', '文案', 'desc', '宝贝'], []) ||
      findAllInputs().find((element) => element.isContentEditable);
    return { title, price, description };
  }

  function hasPublishForm(form = findPublishForm()) {
    return !!(form.price && (form.description || form.title));
  }

  function isPublishSuccessPage() {
    return (
      /\/seller-item\/publish\/success/.test(location.hash) ||
      /发布成功/.test(pageText())
    );
  }

  function findSubmitButton(allowDisabled = false) {
    const pattern = /^(发布|立即发布|确认发布|提交发布|发布宝贝)$/;
    return [...document.querySelectorAll('button, [role="button"], [class*="btn"]')]
      .filter(visible)
      .find((element) => {
        const text = cleanText(element.innerText || element.textContent);
        return text && text.length <= 8 && pattern.test(text) && (allowDisabled || !element.disabled);
      });
  }

  function formErrors() {
    const texts = [
      ...new Set(
        [...document.querySelectorAll('.ant-form-item-explain-error')]
          .map((node) => cleanText(node.innerText))
          .filter(Boolean)
      ),
    ];
    if (texts.length) return texts;
    const matches = pageText().match(/(请填写|请输入|请选择|必填)[^。；\n]{0,30}/g);
    return matches ? [...new Set(matches)] : [];
  }

  function findCategoryItem() {
    return findFormItem('分类');
  }

  function categoryText() {
    const found = findCategoryItem();
    if (!found) return '';
    const selected =
      found.item.querySelector('.ant-select-selection-item') ||
      found.item.querySelector('[class*="selection-item"]');
    return cleanText(selected ? selected.innerText || selected.textContent : '');
  }

  function categoryUnsupported() {
    return /网页版暂不支持发布此分类|网页版暂不支持发布/.test(pageText());
  }

  async function chooseCategory(name) {
    const found = findCategoryItem();
    if (!found) return false;
    const input = found.item.querySelector('input[role="combobox"]');
    const selector = found.item.querySelector('.ant-select-selector') || found.item;

    for (let attempt = 0; attempt < 2; attempt += 1) {
      if (categoryText() === name && !categoryUnsupported()) return true;

      // 闲鱼这个分类选择器对普通 .click() 不响应，需要 focus 后发送 ArrowDown；
      // 下拉列表是虚拟列表，目标项可能在底部，需要把 holder 滚到底部。
      if (input) {
        input.focus();
        if (input.getAttribute('aria-expanded') !== 'true') {
          input.dispatchEvent(
            new KeyboardEvent('keydown', {
              key: 'ArrowDown',
              code: 'ArrowDown',
              keyCode: 40,
              which: 40,
              bubbles: true,
              cancelable: true,
            })
          );
        }
      } else {
        selector.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }));
        selector.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 }));
      }

      await new Promise((resolve) => setTimeout(resolve, 800));
      if (categoryText() === name && !categoryUnsupported()) return true;

      const dropdown = [...document.querySelectorAll('.ant-select-dropdown')].find(visible);
      if (!dropdown) continue;
      const holder = dropdown.querySelector('.rc-virtual-list-holder');
      if (holder) {
        holder.scrollTop = holder.scrollHeight;
        holder.dispatchEvent(new Event('scroll', { bubbles: true }));
        await new Promise((resolve) => setTimeout(resolve, 600));
      }

      const options = [...dropdown.querySelectorAll('.ant-select-item-option')]
        .filter(visible)
        .filter((option) => cleanText(option.innerText) === name || option.getAttribute('title') === name);
      if (!options[0]) continue;
      options[0].dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }));
      options[0].dispatchEvent(new MouseEvent('mouseup', { bubbles: true, cancelable: true, button: 0 }));
      options[0].click();
      await new Promise((resolve) => setTimeout(resolve, 900));
      if (categoryText() === name && !categoryUnsupported()) return true;
    }
    return categoryText() === name && !categoryUnsupported();
  }

  async function ensureCategorySupported() {
    const fallback = '其他闲置';
    const started = Date.now();
    let lastText = '';
    let stableSince = 0;

    // 分类是闲鱼根据图片/描述异步识别的。识别完成前可能短暂出现「发布」按钮可用，
    // 等分类出现并稳定后再判断，避免刚点完发布又被改成不支持网页发布的分类。
    while (Date.now() - started < 20000) {
      if (!findCategoryItem()) {
        if (Date.now() - started > 10000) return { ok: true, category: '' };
        await new Promise((resolve) => setTimeout(resolve, 400));
        continue;
      }
      const current = categoryText();
      if (categoryUnsupported()) break;
      if (current && current !== lastText) {
        lastText = current;
        stableSince = Date.now();
      } else if (current && current === lastText && Date.now() - stableSince >= 1800) {
        if (current !== '教材教辅') return { ok: true, category: current };
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 400));
    }

    const current = categoryText();
    if (!categoryUnsupported() && current && current !== '教材教辅') {
      return { ok: true, category: current };
    }

    appendLog(`当前分类「${current || '未识别'}」网页版不支持，正在切换为「${fallback}」…`, 'warn');
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const switched = await chooseCategory(fallback);
      if (!switched) break;
      await new Promise((resolve) => setTimeout(resolve, 1500));
      if (categoryText() === fallback && !categoryUnsupported()) {
        appendLog(`已切换分类为「${fallback}」；如不接受请手动改回并扫码发布`, 'warn');
        toast('网页版不支持原分类，已切换为「其他闲置」', 'info', 7000);
        return { ok: true, category: fallback, fallback: true };
      }
    }
    return {
      ok: false,
      reason: '当前分类不支持网页发布，自动切换为「其他闲置」失败，请手动选择分类',
    };
  }

  function findConditionOption(condition) {
    if (!condition) return null;
    const variants = new Set([
      condition,
      condition.replace('九成新', '9成新').replace('八成新', '8成新').replace('七成新', '7成新'),
      condition.replace('九成新', '90新').replace('八成新', '80新').replace('七成新', '70新'),
      '二手',
      '轻微使用痕迹',
    ]);
    const candidates = [...document.querySelectorAll('label, [role="radio"], [role="option"], li, button, [class*="option"], [class*="item"]')]
      .filter(visible)
      .filter((element) => cleanText(element.innerText).length <= 12);
    for (const element of candidates) {
      const text = cleanText(element.innerText);
      if (!text) continue;
      for (const variant of variants) {
        if (text === variant || text.includes(variant)) {
          return element;
        }
      }
    }
    return null;
  }

  function setContentEditable(element, value) {
    const text = String(value || '');
    element.focus();
    let inserted = false;
    try {
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(element);
      selection.removeAllRanges();
      selection.addRange(range);
      inserted = document.execCommand('insertText', false, text);
    } catch {}
    if (!inserted || !cleanText(element.innerText)) {
      element.textContent = text;
      element.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
    }
    element.dispatchEvent(new Event('change', { bubbles: true }));
    element.blur();
  }

  function uploadItem() {
    const found = findFormItem('宝贝图片');
    return found ? found.item : null;
  }

  function uploadPreviewCount() {
    const item = uploadItem();
    if (!item) return 0;
    const sortables = [...item.querySelectorAll('[aria-roledescription="sortable"]')].filter(visible);
    if (sortables.length) return sortables.length;
    return [...item.querySelectorAll('img')].filter(
      (image) => visible(image) && image.naturalWidth >= 50 && !/tps-/.test(image.src)
    ).length;
  }

  async function waitForUploads(count, timeout) {
    const target = Math.min(Math.max(count, 0), 9);
    if (!target) return { ok: false, count: 0 };
    const result = await waitFor(() => {
      const item = uploadItem();
      if (!item) return null;
      const previews = [...item.querySelectorAll('[aria-roledescription="sortable"]')].filter(visible);
      const ready = previews.filter((node) =>
        [...node.querySelectorAll('img')].some(
          (image) => visible(image) && image.complete && image.naturalWidth >= 50
        )
      ).length;
      const success = item.className.includes('ant-form-item-has-success');
      const failed = item.className.includes('ant-form-item-has-error');
      if (failed) return { ok: false, count: ready };
      if (ready >= target && (success || !item.className.includes('ant-form'))) {
        return { ok: true, count: ready };
      }
      return null;
    }, timeout, 500);
    return result || { ok: false, count: uploadPreviewCount() };
  }

  async function submitPublish() {
    const submit = findSubmitButton();
    if (!submit) return { ok: false, reason: '没有找到可用的「发布」按钮' };
    submit.click();
    appendLog('已点击发布，等待结果…');
    const outcome = await waitFor(() => {
      // 闲鱼发布成功后会跳到 #/seller-item/publish/success?itemId=...，
      // 有时是整页刷新，原脚本会被中断；页面初始化时也会检查这个地址。
      if (/\/seller-item\/publish\/success/.test(location.hash)) return { done: true };
      const text = pageText();
      if (/发布成功|提交成功|发布完成/.test(text)) return { done: true };
      const dialog = [...document.querySelectorAll('[role="dialog"], .ant-modal')].find(visible);
      if (!dialog) return null;
      const dialogText = cleanText(dialog.innerText);
      if (/扫码|二维码/.test(dialogText)) return { qr: true, text: dialogText.slice(0, 160) };
      if (/验证|滑块|安全/.test(dialogText)) return { verify: true, text: dialogText.slice(0, 160) };
      const confirm = [...dialog.querySelectorAll('button, [role="button"]')]
        .filter(visible)
        .find(
          (element) =>
            /^(确认发布|继续发布|确认|确定)$/.test(cleanText(element.innerText)) && !element.disabled
        );
      if (confirm) {
        confirm.click();
        appendLog('已点击确认发布');
        return null;
      }
      return null;
    }, 25000, 600);
    if (outcome && outcome.done) {
      appendLog('发布成功', 'ok');
      return { ok: true, submitted: true };
    }
    if (outcome && outcome.qr) {
      return { ok: false, reason: '该分类需要扫码到闲鱼 APP 继续发布' };
    }
    if (outcome && outcome.verify) {
      return { ok: true, submitted: true, uncertain: true, reason: '发布后出现人工验证，请在闲鱼确认结果' };
    }
    if (/发布成功|提交成功|发布完成/.test(pageText())) {
      return { ok: true, submitted: true };
    }
    const errors = formErrors();
    if (errors.length) return { ok: false, reason: errors.slice(0, 2).join('；') };
    return { ok: true, submitted: true, uncertain: true };
  }

  async function buildFiles(payload) {
    const files = [];
    const images = (payload.images || []).slice(0, 9);
    for (const image of images) {
      const path = new URL(image.url, API).pathname + (new URL(image.url, API).search || '');
      const blob = await requestBlob(path);
      const type = blob.type || 'image/jpeg';
      const extension = type.includes('png') ? 'png' : type.includes('webp') ? 'webp' : 'jpg';
      files.push(new File([blob], `${payload.listingId}-${image.id}.${extension}`, { type }));
    }
    return files;
  }

  function attachFiles(files) {
    if (!files.length) return { ok: false, reason: '没有可上传的图片' };
    const input = [...document.querySelectorAll('input[type="file"]')].find((element) =>
      String(element.getAttribute('accept') || '').includes('image')
    ) || document.querySelector('input[type="file"]');

    if (input) {
      const transfer = new DataTransfer();
      files.forEach((file) => transfer.items.add(file));
      input.files = transfer.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return { ok: true, count: files.length };
    }

    const dropZone = [...document.querySelectorAll('[class*="upload"], [class*="Upload"], [class*="drop"]')].find(visible);
    if (dropZone) {
      const transfer = new DataTransfer();
      files.forEach((file) => transfer.items.add(file));
      dropZone.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
      return { ok: true, count: files.length };
    }
    return { ok: false, reason: '没有找到图片上传入口' };
  }

  async function waitFor(check, timeout, interval) {
    const started = Date.now();
    while (Date.now() - started < timeout) {
      try {
        const value = check();
        if (value) return value;
      } catch {}
      await new Promise((resolve) => setTimeout(resolve, interval || 400));
    }
    return null;
  }

  async function report(listingId, status, extra = {}) {
    try {
      await requestJson(`/api/listings/${listingId}/report`, {
        method: 'POST',
        body: { status, url: extra.url || location.href, error: extra.error || '' },
      });
    } catch (error) {
      console.warn('[淘书工坊] 回传发布状态失败', error);
    }
  }

  async function fillPublishForm(payload, options = {}) {
    const form = findPublishForm();
    if (!form.price || (!form.description && !form.title)) {
      return { ok: false, reason: '没有找到价格/描述输入框，可能还没进入发布页' };
    }

    if (form.title) {
      setNativeValue(form.title, payload.title);
      appendLog('已填写标题');
    } else {
      appendLog('闲鱼新版发布页没有独立标题，已把标题写到描述首行');
    }

    setNativeValue(form.price, String(payload.price));
    appendLog('已填写价格');

    if (form.description) {
      let text = payload.description || '';
      if (!form.title && payload.title) text = `${payload.title}\n\n${text}`;
      if (text.length > 1500) {
        text = text.slice(0, 1500);
        appendLog('描述超过 1500 字，已截断', 'warn');
      }
      if (form.description.isContentEditable) setContentEditable(form.description, text);
      else setNativeValue(form.description, text);
      appendLog('已填写描述');
    } else {
      appendLog('未找到描述输入框', 'err');
      return { ok: false, reason: '没有找到描述输入框，请手动填写描述' };
    }

    const condition = findConditionOption(payload.condition);
    if (condition && !condition.classList.contains('active') && !condition.classList.contains('selected')) {
      condition.click();
      appendLog(`已选择成色：${payload.condition}`);
    }

    const images = Array.isArray(payload.images) ? payload.images : [];
    if (!images.length) {
      return { ok: false, reason: '当前没有图片，请先在书库里补图再发布' };
    }

    const existing = uploadPreviewCount();
    if (existing >= images.length) {
      appendLog(`页面已有 ${existing} 张图片，跳过重复上传`);
    } else if (existing > 0) {
      return {
        ok: false,
        reason: `页面已有 ${existing} 张图片，少于需要上传的 ${images.length} 张。请手动删除旧图后重试。`,
      };
    } else {
      let fileResult = { ok: false, reason: '没有上传图片' };
      try {
        setPanelBody('<div class="progress"><i></i></div>');
        appendLog(`正在上传 ${images.length} 张图片…`);
        const files = await buildFiles(payload);
        fileResult = attachFiles(files);
        if (fileResult.ok) appendLog(`已提交 ${fileResult.count} 张图片，等待上传完成…`, 'ok');
        else appendLog(fileResult.reason, 'err');
      } catch (error) {
        fileResult = { ok: false, reason: '图片上传失败：' + (error.message || error) };
        appendLog(fileResult.reason, 'err');
      }
      if (!fileResult.ok) {
        return { ok: false, reason: fileResult.reason || '图片上传失败，请手动上传图片' };
      }
      const uploaded = await waitForUploads(images.length, 30000);
      if (!uploaded.ok) {
        const reason =
          uploaded.count > 0
            ? `图片只上传成功 ${uploaded.count}/${images.length} 张，请检查网络或手动上传`
            : '图片上传超时，请检查网络或手动上传';
        appendLog(reason, 'err');
        return { ok: false, reason };
      }
      appendLog(`图片上传完成（${uploaded.count} 张）`, 'ok');
    }

    const category = await ensureCategorySupported();
    if (!category.ok) return { ok: false, reason: category.reason };

    const submit = await waitFor(() => findSubmitButton(), options.autoSubmit ? 20000 : 8000, 500);
    if (!submit) {
      const errors = formErrors();
      const reason = errors.length
        ? `发布按钮仍不可用：${errors.slice(0, 2).join('；')}`
        : '发布按钮仍不可用，请手动检查表单（分类、图片、价格、描述）';
      appendLog(reason, 'err');
      return { ok: false, reason };
    }

    // 分类识别是异步的：按钮短暂可用后再等一会儿，避免刚返回就被改成不支持网页发布的分类。
    await new Promise((resolve) => setTimeout(resolve, 1500));
    if (categoryUnsupported()) {
      const retry = await ensureCategorySupported();
      if (!retry.ok) return { ok: false, reason: retry.reason || '分类切换失败' };
    }
    const stableSubmit = await waitFor(() => findSubmitButton(), 3000, 300);
    if (!stableSubmit) {
      const errors = formErrors();
      const reason = errors.length
        ? `发布按钮仍不可用：${errors.slice(0, 2).join('；')}`
        : '发布按钮仍不可用，请手动检查表单（分类、图片、价格、描述）';
      appendLog(reason, 'err');
      return { ok: false, reason };
    }
    appendLog('表单校验通过，发布按钮已可用', 'ok');

    if (options.autoSubmit) return await submitPublish();
    return { ok: true, submitted: false };
  }

  async function completePublishTask(task) {
    state.task = task;
    openPanel('闲鱼发布任务', '发布成功');
    setPanelBody('<ul class="log"></ul>');
    appendLog('闲鱼已返回发布成功页面，商品已发布', 'ok');
    await report(task.id, 'published');
    clearTask();
    toast('闲鱼发布成功', 'success', 7000);
    refreshSiteButton();
  }

  async function runPublishTask(task) {
    if (state.busy) return;
    state.task = task;
    if (isPublishSuccessPage()) {
      setBusy(true);
      try {
        await completePublishTask(task);
      } finally {
        setBusy(false);
        refreshSiteButton();
      }
      return;
    }
    setBusy(true);
    openPanel('闲鱼发布任务', `任务 #${task.id}`);
    setPanelBody('<div class="progress"><i></i></div>');

    try {
      const payload = await requestJson(`/api/listings/${task.id}/payload`);
      panelTitle.textContent = '闲鱼发布任务';
      panelSub.textContent = `¥${payload.price} · ${payload.title.slice(0, 16)}`;
      setPanelBody('<ul class="log"></ul>');
      appendLog(`准备发布：${payload.title}`);
      await report(task.id, 'filling');

      // 闲鱼的登录页把内容放在 passport.goofish.com 的跨域 iframe 里，
      // 顶层 document.body.innerText 可能是空的，所以同时检查登录 iframe / 登录路由。
      const loginFrame = [...document.querySelectorAll('#alibaba-login-box, iframe[src*="mini_login"]')].find(visible);
      const loginRoute = /\/login(?:[/?#]|$)/.test(location.hash);
      const loggedOut =
        !!loginFrame ||
        loginRoute ||
        /扫码登录|请登录|登录后|密码登录|快速进入|扫码安全登录/.test(
          cleanText(document.body ? document.body.innerText.slice(0, 3000) : '')
        );
      if (loggedOut) {
        appendLog('检测到闲鱼登录页，请先点击「快速进入」或扫码登录，登录后再点「继续填表」', 'err');
        panelSub.textContent = '等待登录';
        await report(task.id, 'failed', { error: '未登录闲鱼' });
        showManualPanel(
          payload,
          '检测到闲鱼登录页。请先点击页面中的「快速进入」或扫码登录，登录后点「再试一次自动填表」。'
        );
        return;
      }

      const ready = await waitFor(() => (hasPublishForm() ? true : null), 12000, 500);
      if (!ready) {
        appendLog('当前不是发布页，正在查找「发布」入口…');
        const entry = findPublishEntry();
        if (entry) {
          entry.click();
          appendLog('已点击发布入口，等待表单加载…');
          const loaded = await waitFor(() => (hasPublishForm() ? true : null), 20000, 600);
          if (loaded) {
            // 继续走后面的填表流程
          } else if (SITE === 'goofish' && !location.hash.includes('/seller-item/publish')) {
            appendLog('入口没有打开发布表单，直接切换到商品发布页…');
            const target = new URL(location.href);
            if (!target.searchParams.has('site')) target.searchParams.set('site', 'COMMONPRO');
            target.hash = '/seller-item/publish';
            location.replace(target.toString());
            return;
          } else {
            appendLog('发布表单没有出现，请手动进入发布页后点「继续填充」', 'err');
            panelSub.textContent = '等待发布页';
            await report(task.id, 'failed', { error: '发布表单未出现' });
            showManualPanel(payload, '发布表单没有出现。请手动打开发布页，然后点「再试一次自动填表」。');
            return;
          }
        } else if (SITE === 'goofish' && !location.hash.includes('/seller-item/publish')) {
          appendLog('当前是卖家中心首页，直接打开商品发布页…');
          const target = new URL(location.href);
          if (!target.searchParams.has('site')) target.searchParams.set('site', 'COMMONPRO');
          target.hash = '/seller-item/publish';
          location.replace(target.toString());
          return;
        } else {
          appendLog('没有找到发布入口，请手动进入「发布闲置」页面', 'err');
          panelSub.textContent = '需要手动进入发布页';
          await report(task.id, 'failed', { error: '未找到发布入口' });
          showManualPanel(payload, '没有找到「发布闲置」入口。请手动打开发布页，然后点「再试一次自动填表」。');
          return;
        }
      }

      const result = await fillPublishForm(payload, { autoSubmit: payload.autoSubmit });
      if (!result.ok) {
        appendLog(result.reason || '填充失败', 'err');
        panelSub.textContent = '需要处理';
        await report(task.id, 'failed', { error: result.reason || '自动填表失败' });
        showManualPanel(payload, result.reason || '自动填表失败，请手动复制内容。');
        return;
      }

      if (result.submitted) {
        if (result.reason) appendLog(result.reason, 'warn');
        panelSub.textContent = result.uncertain ? '已提交（请确认）' : '已提交';
        await report(task.id, result.uncertain ? 'filling' : 'published');
        toast(result.uncertain ? '已提交，请在闲鱼确认结果' : '闲鱼发布成功', 'success', 7000);
        if (!result.uncertain) clearTask();
      } else {
        panelSub.textContent = '已填好，等待手动提交';
        await report(task.id, 'filling');
        toast('已填好表单，请检查后点击发布', 'success', 6000);
      }
    } catch (error) {
      appendLog(error.message || '发布任务失败', 'err');
      panelSub.textContent = '任务失败';
      await report(task.id, 'failed', { error: error.message || '任务失败' });
      toast(error.message || '发布任务失败', 'error');
    } finally {
      setBusy(false);
      refreshSiteButton();
    }
  }

  async function runGoofishAction() {
    const task = loadTask();
    if (!task) {
      toast('没有待发布任务，请从工作台点「发布」', 'error');
      return;
    }
    await runPublishTask(task);
  }

  /* ------------------------------ 工具条控制 ----------------------------- */

  function refreshSiteButton() {
    if (SITE === 'taobao') {
      siteButton.textContent = state.busy ? (lowestRequest ? '比价中…' : '抓取中…') : lowestRequest ? '继续比价' : sourceSearch ? '采集货源候选' : '抓取本页';
    } else if (SITE === 'kongfz') {
      if (bookLookup) { siteButton.textContent = state.busy ? '检索中…' : '重试书目检索'; return; }
      if (state.context) {
        siteButton.textContent = `导入到《${(state.context.title || '').slice(0, 8)}》`;
        siteButton.classList.add('accent');
      } else {
        siteButton.textContent = '选图导入';
        siteButton.classList.remove('accent');
      }
    } else if (SITE === 'goofish') {
      siteButton.textContent = state.busy ? '处理中…' : state.task ? '继续填表' : '开始填表';
    } else {
      siteButton.textContent = '淘书助手';
    }
  }

  async function runSiteAction() {
    if (sourceSearch && lowestRequest) return runLowestPrice(true);
    if (sourceSearch) {
      if (!(await collectTaobaoSearch())) toast('未识别到商品，请登录或等待结果加载后重试，也可进入商品详情抓取', 'info');
      return;
    }
    if (bookLookup) return searchBookMetadata();
    if (SITE === 'taobao') return captureTaobao();
    if (SITE === 'kongfz') return runKongfzAction();
    if (SITE === 'goofish') return runGoofishAction();
    openPanel('淘书助手', SITE_LABEL);
    setPanelBody('<p class="muted">当前页面不是支持的站点。可访问淘宝、孔夫子、闲鱼卖家中心。</p>');
  }

  async function checkOnline() {
    try {
      const health = await requestJson('/api/health');
      state.online = !!health.ok;
    } catch {
      state.online = false;
    }
    onlineDot.classList.toggle('on', state.online);
    onlineDot.title = state.online ? '本地服务已连接' : '本地服务未连接';
  }

  function initAddressSync() {
    setInterval(() => {
      if (document.activeElement === addrInput) return;
      if (addrInput.value !== location.href) addrInput.value = location.href;
      if (state.address !== location.href) {
        state.address = location.href;
        addrInput.value = location.href;
      }
    }, 1200);
  }

  async function init() {
    setCollapsed(state.collapsed);
    refreshSiteButton();
    initAddressSync();
    await checkOnline();

    if (SITE === 'taobao') {
      if (sourceSearch && lowestRequest) { await runLowestPrice(); return; }
      if (sourceSearch) {
        let attempts = 0;
        const timer = setInterval(async () => {
          if (++attempts > 20) {
            clearInterval(timer);
            toast('搜索结果未就绪，请登录或调整检索词后点击采集货源候选', 'info');
          } else if (await collectTaobaoSearch()) clearInterval(timer);
        }, 1500);
        window.addEventListener('pagehide', () => clearInterval(timer), {once:true});
      }
      setPanelBody(
        sourceSearch ? '<p class="muted">正在读取当前搜索结果；登录后可点击「采集货源候选」重试。</p>' : '<p class="muted">打开商品详情页后点「抓取本页」，保存为待核验货源。</p>'
      );
    }
    if (SITE === 'kongfz') {
      if (bookLookup) { await searchBookMetadata(); return; }
      const context = await loadImportContext();
      if (context) {
        toast(`找图目标：《${context.title}》`, 'info', 5000);
      }
    }
    if (SITE === 'goofish') {
      const task = loadTask();
      if (task) {
        if (isPublishSuccessPage()) {
          await completePublishTask(task);
        } else {
          await runPublishTask(task);
        }
      } else {
        setPanelBody('<p class="muted">等待工作台下发的发布任务。也可以先在这里登录闲鱼。</p>');
      }
    }

    window.addEventListener('error', (event) => {
      if (!event || !event.message) return;
      if (/Taoshu|taoshu/i.test(event.message)) console.warn('[淘书工坊]', event.message);
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init, { once: true });
  } else {
    init();
  }
  }

  // Tauri 的初始化脚本可能早于 <html> 出现，等 documentElement 可用后再建工具条。
  if (document.documentElement) {
    boot();
  } else {
    const timer = setInterval(() => {
      if (!document.documentElement) return;
      clearInterval(timer);
      boot();
    }, 10);
    document.addEventListener(
      'DOMContentLoaded',
      () => {
        clearInterval(timer);
        boot();
      },
      { once: true },
    );
  }
})();
