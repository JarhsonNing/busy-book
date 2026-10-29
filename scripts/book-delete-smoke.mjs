// Isolated HTTP acceptance; --serve keeps synthetic books available for UI review.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {DatabaseSync} from 'node:sqlite';
const root=await fs.mkdtemp(path.join(os.tmpdir(),'taoshu-delete-'));
const base='http://127.0.0.1:18997';
const server=spawn(path.resolve('src-tauri/target/debug/api-server.exe'),[],{env:{...process.env,PORT:'18997',TAOSHU_DATA:root},windowsHide:true,stdio:'ignore'});
let token;
async function request(route,method='GET',body){
  const res=await fetch(base+'/api'+route,{method,headers:{'content-type':'application/json','x-kf-token':token||''},body:body===undefined?undefined:JSON.stringify(body)});
  return {status:res.status,value:await res.json()};
}
async function api(route,method,body){const res=await request(route,method,body);assert.equal(res.status,200,JSON.stringify(res.value));return res.value;}
try{
  for(let n=0;n<100;n++){try{const r=await fetch(base+'/api/token');if(r.ok){token=(await r.json()).token;break;}}catch{}await new Promise(r=>setTimeout(r,100));}
  assert.ok(token,'isolated API did not start');
  const draft=await api('/books','POST',{title:'测试 · 可删除的未发布图书',stock:0});
  const published=await api('/books','POST',{title:'测试 · 已发布图书',stock:0});
  const db=new DatabaseSync(path.join(root,'shelf.db'));
  db.prepare("UPDATE books SET status='listed' WHERE id=?").run(published.id);
  db.close();
  const preview=await api('/books/delete-preview','POST',{ids:[draft.id,published.id]});
  assert.equal(preview.items[0].canDelete,true);
  assert.equal(preview.items[1].canDelete,false);
  assert.equal((await request('/books/'+published.id,'DELETE')).status,400);
  const deleted=await api('/books/delete','POST',{ids:[draft.id,published.id,draft.id]});
  assert.deepEqual(deleted.deletedIds,[draft.id]);
  assert.equal(deleted.skipped.length,1);
  assert.equal((await request('/books/'+draft.id)).status,404);
  assert.equal((await api('/books/'+published.id)).id,published.id);
  assert.equal((await request('/books/delete','POST',{ids:[]})).status,400);
  const single=await api('/books','POST',{title:'测试 · 单本删除'});
  assert.equal((await api('/books/'+single.id,'DELETE')).deleted,true);
  const unauthorized=await fetch(base+'/api/books/delete',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({ids:[published.id]})});
  assert.equal(unauthorized.status,401);
  const ui=await api('/books','POST',{title:'测试 · 数据库系统概论（未发布）',author:'测试作者',edition:'第5版',isbn:'9787040406641',stock:0});
  await api('/books','POST',{title:'测试 · 高等数学（未发布）',author:'测试作者',edition:'第7版',stock:0});
  const library=await api('/commerce/library?page=1&pageSize=30');
  assert.equal(library.items.find(b=>b.id===ui.id).canDelete,true);
  assert.equal(library.items.find(b=>b.id===published.id).canDelete,false);
  // Verify a stale last page clamps after deletion without losing the filter.
  const last=await api('/books','POST',{title:'分页验收 · 末页'});
  await api('/books/delete','POST',{ids:[last.id]});
  const page=await api('/commerce/library?q='+encodeURIComponent('分页验收')+'&page=2&pageSize=1');
  assert.equal(page.total,0);assert.equal(page.page,1);
  console.log('PASS: delete preview, guarded legacy DELETE, mixed/duplicate batch, validation, authentication, list eligibility and last-page clamp. Synthetic data only.');
  if(process.argv.includes('--serve')){
    console.log('TEST_API: '+base+'; TEST_DATA: '+root);
    await new Promise(resolve=>{process.once('SIGINT',resolve);process.once('SIGTERM',resolve);process.stdin.once('data',resolve);process.stdin.resume();});
    process.stdin.pause();
  }
}finally{
  if(server.exitCode==null){const stopped=once(server,'exit');server.kill();await stopped;}
  assert.equal(path.dirname(path.resolve(root)),path.resolve(os.tmpdir()));
  assert.ok(path.basename(root).startsWith('taoshu-delete-'));
  await fs.rm(root,{recursive:true,force:true,maxRetries:8,retryDelay:250});
}
