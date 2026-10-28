import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import vm from 'node:vm';

const source = await fs.readFile(new URL('../src-tauri/inject/browser.js', import.meta.url), 'utf8');
const start = source.indexOf('  function extractTaobaoSearchCandidates(');
const end = source.indexOf('  let bookLookup', start);
assert.ok(start > 0 && end > start);
function anchor(id, host = 'item.taobao.com', title = '数学分析 第3版', integer = '12', fraction = '.50', label = '') {
  const card = { querySelector(selector) {
    if (selector.includes('title')) return {textContent:title};
    if (selector.includes('priceInt')) return {textContent:integer};
    if (selector.includes('priceFloat')) return {textContent:fraction};
    if (selector.includes('priceDesc')) return {textContent:label};
    if (selector.includes('price')) return {textContent:'¥ '+integer+fraction+'0人付款'};
    if (selector.includes('shopName')) return {textContent:'教材店'};
    return null;
  }};
  return {href:`https://${host}/item.htm?id=${id}`,closest:()=>card,getAttribute:()=>null};
}
function context(anchors) {
  const sent = [];
  const state = {busy:false};
  return {sent, state, sourceBookId:17, URL, Date, Map,
    lookupParams: new URLSearchParams({q:'数学分析 第3版'}),location:{href:'https://s.taobao.com/search'},
    document:{querySelectorAll:selector=>selector.startsWith('a[')?anchors:[]},setBusy(value){state.busy=value;},
    async requestJson(url, options){sent.push({url,...options});return {};},toast(){},refreshSiteButton(){},
  };
}
test('Taobao search extracts actual cards, rejects foreign links and deduplicates products', () => {
  const c = context([anchor('1'),anchor('1'),anchor('2','detail.tmall.com'),anchor('3','item.taobao.com.evil.test'),anchor('4','item.taobao.com','')]);
  const items = vm.runInNewContext(source.slice(start,end)+'\nextractTaobaoSearchCandidates()',c);
  assert.equal(items.length,2);
  assert.equal(items[0].sourceBookId,17);
  assert.equal(items[0].price,12.5);
  assert.equal(items[0].title,'数学分析 第3版');
  assert.equal(items[1].url,'https://detail.tmall.com/item.htm?id=2');
});
test('collect sends source-only captures to the right book and preserves unknown freight/specification', async () => {
  const c = context([anchor('1')]);
  assert.equal(await vm.runInNewContext(source.slice(start,end)+'\ncollectTaobaoSearch()',c),true);
  assert.equal(c.sent.length,1);
  assert.equal(c.sent[0].body.asSource,true);
  assert.equal(c.sent[0].body.sourceBookId,17);
  assert.equal(c.sent[0].body.specs,undefined);
  assert.equal(c.sent[0].body.freight,undefined);
  assert.equal(c.state.busy,false);
});
test('login/no product page never reports successful empty collection', async () => {
  const c = context([]);
  assert.equal(await vm.runInNewContext(source.slice(start,end)+'\ncollectTaobaoSearch()',c),false);
  assert.equal(c.sent.length,0);
});

test('supplied split-price structure preserves decimals, integer prices and subsidy labels', () => {
  const c = context([anchor('1','item.taobao.com','模拟补贴商品','52','.48','补贴后'),anchor('2','item.taobao.com','模拟整数价','19','')]);
  const items = vm.runInNewContext(source.slice(start,end)+'\nextractTaobaoSearchCandidates()',c);
  assert.equal(items[0].price,52.48); assert.equal(items[0].priceLabel,'补贴后');
  assert.equal(items[1].price,19); // Never joins the following 0人付款 into 190.
});

function priceContext(frames) {
  const c = context(frames[0] || []);
  let frame = 0;
  c.lowestRequest = 'active-request';
  c.navigations = []; c.focused = 0; c.panelSub = {}; c.scrolls = 0; c.clicks = 0;
  c.openPanel = () => {};
  const root = {scrollHeight:1000,clientHeight:1000,scrollTop:0,scrollTo(){c.scrolls++;frame=Math.min(frame+1,frames.length-1);}};
  c.document.scrollingElement = c.document.documentElement = root;
  c.document.querySelector = () => null;
  c.document.querySelectorAll = selector => selector.startsWith('a[') ? frames[frame] || [] : selector.startsWith('.next-pagination') ? c.next ? [c.next] : [] : c.verification ? [{getClientRects:()=>[{}]}] : [];
  c.window = {innerHeight:1000};
  c.setTimeout = callback => {callback(); return 1;};
  c.focusDashboard = async () => {c.focused++;};
  c.location.assign = url => c.navigations.push(url);
  c.task = {requestId:c.lowestRequest,state:'searching',pagesDone:0,batchesDone:0,maxItems:180,items:[]};
  c.requestJson = async (url, options) => {
    if (!options) return c.task;
    c.sent.push({url,...options});
    const body=options.body;
    if (body.action==='batch') {
      const items=[...c.task.items,...body.items];
      c.task={...c.task,items,count:items.length,pagesDone:body.batch,batchesDone:body.batch,state:body.batch===3?'completed':'searching'};
    } else if (body.action==='finish') c.task={...c.task,state:'completed'};
    else if (body.action==='resume') c.task={...c.task,state:'searching'};
    else c.task={...c.task,state:'waiting'};
    return c.task;
  };
  return c;
}
const cards = (first,count) => Array.from({length:count},(_,i)=>anchor(String(first+i)));
const run = c => vm.runInNewContext(source.slice(start,end)+'\nrunLowestPrice()',c);

test('scroll-loading collects newly appended cards beyond the first 60, without invented page URLs', async () => {
  const c = priceContext([cards(1,40),cards(1,80),cards(1,120)]);
  await run(c);
  const batches=c.sent.filter(r=>r.body.action==='batch');
  assert.deepEqual(batches.map(r=>r.body.items.length),[60,60]);
  assert.equal(c.task.count,120);assert.equal(new Set(c.task.items.map(i=>i.itemId)).size,120);
  assert.equal(c.task.state,'completed');assert.equal(c.sent.at(-1).body.action,'finish');
  assert.equal(c.navigations.length,0);assert.equal(c.focused,1);assert.ok(c.scrolls>0);
});
test('virtualized cards are retained across scrolls and a partial batch is saved at the visible end', async () => {
  const c = priceContext([cards(1,30),cards(31,30),cards(61,30)]);
  await run(c);
  assert.equal(c.task.count,90);assert.deepEqual(c.sent.filter(r=>r.body.action==='batch').map(r=>r.body.items.length),[60,30]);
});
test('the collection limit stops at 180 candidates without further scrolling or navigation', async () => {
  const c = priceContext([cards(1,240)]);
  await run(c);
  assert.equal(c.task.count,180);assert.equal(c.task.state,'completed');
  assert.equal(c.sent.length,3);assert.equal(c.navigations.length,0);assert.equal(c.scrolls,0);
});
test('a real main-results next button is clicked only after scrolling and waiting for new cards', async () => {
  const c = priceContext([cards(1,20)]);
  c.next={getClientRects:()=>[{}],getAttribute:()=>null,click(){c.clicks++;framesToLoad();}};
  function framesToLoad(){c.document.querySelectorAll=selector=>selector.startsWith('a[')?cards(21,20):[];}
  await run(c);
  assert.equal(c.clicks,1);assert.equal(c.task.count,40);assert.equal(c.task.state,'completed');
  assert.equal(c.navigations.length,0);assert.ok(c.scrolls>=6);
});
test('an unresponsive next button pauses instead of repeatedly clicking and skipping results', async () => {
  const c = priceContext([cards(1,20)]);
  c.next={getClientRects:()=>[{}],getAttribute:()=>null,click(){c.clicks++;}};
  await run(c);
  assert.equal(c.clicks,1);assert.equal(c.task.count,20);assert.equal(c.task.state,'waiting');
  assert.match(c.sent.at(-1).body.error,/尚未加载/);
});
test('resume scrolls past already saved results and only appends new products', async () => {
  const c = priceContext([cards(1,40),cards(1,80)]);
  c.task.items=cards(1,40).map(a=>({url:a.href}));c.task.batchesDone=c.task.pagesDone=1;
  await run(c);
  assert.equal(c.task.count,80);assert.equal(c.sent[0].body.batch,2);assert.equal(c.sent[0].body.items.length,40);
});
test('no visible results pauses price search instead of declaring completion or navigating', async () => {
  const c = priceContext([[]]);
  await run(c);
  assert.equal(c.sent.length,1);assert.equal(c.sent[0].body.action,'waiting');
  assert.ok(c.sent[0].body.error);assert.equal(c.navigations.length,0);
});
test('canceled and superseded searches never submit a batch', async () => {
  for (const kind of ['canceled','replaced']) {
    const c = priceContext([cards(1,20)]);
    if (kind==='canceled') c.task.state='canceled';
    else {let reads=0;c.requestJson=async()=>++reads===1?c.task:{...c.task,requestId:'new-request'};}
    await run(c);
    assert.equal(c.sent.length,0);assert.equal(c.navigations.length,0);
  }
});
test('visible login or verification overlays pause collection even when underlying cards remain', async () => {
  const c = priceContext([cards(1,20)]);c.verification=true;
  await run(c);
  assert.equal(c.sent.length,1);assert.equal(c.sent[0].body.action,'waiting');
  assert.match(c.sent[0].body.error,/安全验证/);assert.equal(c.navigations.length,0);
});
test('a verification overlay appearing mid-scroll preserves the candidates already read', async () => {
  const c = priceContext([cards(1,20)]);
  const scroll = c.document.scrollingElement.scrollTo;
  c.document.scrollingElement.scrollTo = () => {scroll();c.verification=true;};
  await run(c);
  assert.deepEqual(c.sent.map(r=>r.body.action),['batch','waiting']);
  assert.equal(c.task.count,20);assert.equal(c.task.state,'waiting');
});
test('resuming on an empty page does not mark previously saved results as a completed search', async () => {
  const c = priceContext([[]]);
  c.task.items=cards(1,20).map(a=>({url:a.href}));c.task.batchesDone=c.task.pagesDone=1;
  await run(c);
  assert.equal(c.sent.length,1);assert.equal(c.sent[0].body.action,'waiting');
  assert.equal(c.task.items.length,20);
});
