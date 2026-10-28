// Isolated acceptance. Website pages are simulated; no account or marketplace is changed.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
const dir = await fs.mkdtemp(path.join(os.tmpdir(),'taoshu-price-'));
const base = 'http://127.0.0.1:18995';
let server, token;
async function boot() {
  server = spawn(path.resolve('src-tauri/target/debug/api-server.exe'),[],{env:{...process.env,PORT:'18995',TAOSHU_DATA:dir},windowsHide:true,stdio:'ignore'});
  for (let n=0;n<100;n++) {
    try { const r=await fetch(base+'/api/token'); if(r.ok){token=(await r.json()).token; return;} } catch {}
    await new Promise(r=>setTimeout(r,100));
  }
  throw new Error('isolated API did not start');
}
async function stop() { if(server && server.exitCode==null){const ended=once(server,'exit');server.kill();await ended;} }
async function api(route, body, expected=200) {
  const r=await fetch(base+'/api'+route,{method:body===undefined?'GET':'POST',headers:{'x-kf-token':token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});
  assert.equal(r.status,expected,route); return r.json();
}
try {
  await boot();
  const book=await api('/books',{title:'模拟用书',isbn:'9787040406641',edition:'第5版',condition:'九成新',stock:0});
  const endpoint='/commerce/books/'+book.id+'/price-search';
  let run=await api(endpoint,{action:'start'}); const requestId=run.requestId;
  const u=new URL(run.searchUrl);assert.equal(u.searchParams.get('sort'),'price-asc');assert.equal(u.searchParams.get('q'),'9787040406641');
  const item=(id,price)=>({url:`https://item.taobao.com/item.htm?id=${id}`,title:'模拟第5版教材',shop:'模拟供应店',price});
  run=await api(endpoint,{action:'batch',requestId,batch:1,items:[item(1,0.5),item(1,0.5),item(2,12)]});
  assert.equal(run.count,2);assert.equal(new URL(run.searchUrl).searchParams.has('taoshuPricePage'),false);assert.equal(new URL(run.searchUrl).searchParams.has('s'),false);assert.equal(run.mode,'scroll');assert.equal(run.bestVerified,null);
  const source=run.rows.find(r=>r.source.data.url.includes('id=2')).source;
  await api('/commerce/sources',{id:source.id,bookId:book.id,data:{...source.data,price:12,freight:3,sku:'第5版',condition:'九成新',dropship:true,status:'available',checkedAt:new Date().toISOString()}});
  run=await api(endpoint);assert.equal(run.bestVerified.total,15);assert.equal(run.bestDisplay.displayPrice,0.5);
  run=await api(endpoint,{action:'batch',requestId,batch:2,items:[item(1,0.5)]});assert.equal(run.state,'waiting');
  await api(endpoint,{action:'resume',requestId});
  await stop();await boot();
  run=await api(endpoint);assert.equal(run.state,'waiting');assert.equal(run.pagesDone,1);assert.equal(run.bestVerified.total,15);
  await api(endpoint,{action:'resume',requestId});
  await api(endpoint,{action:'batch',requestId,batch:2,items:[item(3,5)]});
  run=await api(endpoint,{action:'batch',requestId,batch:3,items:[item(4,7)]});assert.equal(run.state,'completed');assert.equal(run.count,4);
  await api(endpoint,{action:'batch',requestId,batch:3,items:[item(4,7)]});
  const newer=await api(endpoint,{action:'start'});
  await api(endpoint,{action:'batch',requestId,batch:1,items:[item(9,1)]},400);
  await api(endpoint,{action:'cancel',requestId:newer.requestId});
  await api(endpoint,{action:'batch',requestId:newer.requestId,batch:1,items:[item(9,1)]},400);
  const finalRun=await api(endpoint,{action:'start'});
  await api(endpoint,{action:'batch',requestId:finalRun.requestId,batch:1,items:[{...item(7,52.48),priceLabel:'补贴后'}]});
  run=await api(endpoint,{action:'finish',requestId:finalRun.requestId,reason:'visible-end'});
  assert.equal(run.completionReason,'visible-end');assert.equal(run.rows.find(r=>r.observed).priceLabel,'补贴后');
  const books=await api('/commerce/library?pageSize=30');assert.equal(books.total,1);
  console.log('PASS: sorted search URL, scroll-batch deduplication and completion, verified freight ranking, pause/restart/resume, cancellation, stale callbacks, no duplicate books');
} finally {
  await stop();
  assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));
  assert.ok(path.basename(dir).startsWith('taoshu-price-'));
  await fs.rm(dir,{recursive:true,force:true});
}
