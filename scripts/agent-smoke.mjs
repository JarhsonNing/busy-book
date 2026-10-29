// Isolated MCP acceptance. --install verifies the official dedicated binary; --serve keeps the test API alive.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import readline from 'node:readline';
import {deflateSync} from 'node:zlib';
const root=await fs.mkdtemp(path.join(os.tmpdir(),'taoshu-agents-'));
const base='http://127.0.0.1:18997';
const server=spawn(path.resolve('src-tauri/target/debug/api-server.exe'),[],{env:{...process.env,PORT:'18997',TAOSHU_DATA:root},windowsHide:true,stdio:'ignore'});
let token,bridge;
const wait=ms=>new Promise(r=>setTimeout(r,ms));
async function api(route,body){const res=await fetch(base+'/api'+route,{method:body===undefined?'GET':'POST',headers:{'x-kf-token':token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const value=await res.json();assert.ok(res.ok,JSON.stringify(value));return value;}
function png(){
  const crc=bytes=>{let n=0xffffffff;for(const byte of bytes){n^=byte;for(let i=0;i<8;i++)n=(n>>>1)^((n&1)?0xedb88320:0);}return (n^0xffffffff)>>>0;};
  const chunk=(type,data)=>{const content=Buffer.concat([Buffer.from(type),data]),size=Buffer.alloc(4),hash=Buffer.alloc(4);size.writeUInt32BE(data.length);hash.writeUInt32BE(crc(content));return Buffer.concat([size,content,hash]);};
  const header=Buffer.alloc(13);header.writeUInt32BE(2,0);header.writeUInt32BE(2,4);header[8]=8;header[9]=6;
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(Buffer.alloc(18))),chunk('IEND',Buffer.alloc(0))]);
}
try{
  for(let i=0;i<100;i++){try{const r=await fetch(base+'/api/token');if(r.ok){token=(await r.json()).token;break;}}catch{}await wait(100);}assert.ok(token,'API did not start');
  const localBefore=await api('/agents/opencode');
  assert.ok(localBefore.local.some(item=>item.available),'installed OpenCode must be detected');
  let connection=await api('/agents/mcp',{enabled:true});
  const config=connection.configs.stdio.mcpServers.taoshu;
  bridge=spawn(config.command,config.args,{windowsHide:true,stdio:['pipe','pipe','pipe']});
  let nextId=0,stderr='';const pending=new Map();
  bridge.stderr.on('data',c=>stderr+=c);
  readline.createInterface({input:bridge.stdout}).on('line',line=>{const result=JSON.parse(line);const p=pending.get(result.id);if(p){clearTimeout(p.timer);pending.delete(result.id);p.resolve(result);}});
  const rpc=(method,params)=>new Promise((resolve,reject)=>{const id=++nextId;const timer=setTimeout(()=>reject(Error('MCP timed out: '+method)),30000);pending.set(id,{resolve,timer});bridge.stdin.write(JSON.stringify({jsonrpc:'2.0',id,method,params})+'\n');});
  assert.equal((await rpc('initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'taoshu-smoke',version:'1'}})).result.protocolVersion,'2025-11-25');
  bridge.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})+'\n');
  assert.equal((await rpc('tools/list',{})).result.tools.length,9);
  const call=async(name,args)=>{const reply=await rpc('tools/call',{name,arguments:args});assert.ok(!reply.error,JSON.stringify(reply.error));assert.equal(reply.result.isError,false,JSON.stringify(reply.result));return reply.result.structuredContent;};
  const book={source:'smoke-agent',externalId:'book-1',sourceUrl:'https://example.org/official/books',title:'模拟 Agent 导入图书',author:'模拟作者',publisher:'模拟出版社',isbn:'9787040406641',edition:'第5版',volume:'上册',publicationYear:'2014'};
  const saved=await call('taoshu_import_book',book);assert.equal((await call('taoshu_import_book',book)).reused,true);
  const batchBook={...book,externalId:'batch-1',edition:'第6版'};
  const batch=await call('taoshu_import_books',{books:[batchBook,{...batchBook,externalId:'batch-invalid',isbn:'bad'},batchBook]});
  assert.equal(batch.created,1);assert.equal(batch.reused,1);assert.equal(batch.failed,1);
  const {DatabaseSync}=await import('node:sqlite');const fixtureDb=new DatabaseSync(path.join(root,'shelf.db'));
  const fixtureTime=new Date().toISOString(),expiry=Math.floor(Date.now()/1000)+3600;
  const fixtureItem={subjectId:'987',title:'模拟豆瓣书目',author:'模拟作者',isbn:'9787040406641',sourceUrl:'https://book.douban.com/subject/987/'};
  const putCache=fixtureDb.prepare('INSERT INTO agent_isbn_cache(cache_key,data,expires_at,updated_at) VALUES(?,?,?,?)');
  putCache.run('query:模拟豆瓣书目',JSON.stringify({state:'candidates',items:[fixtureItem],fetchedAt:fixtureTime,testFixture:true}),expiry,fixtureTime);
  putCache.run('subject:987',JSON.stringify({state:'found',item:fixtureItem,fetchedAt:fixtureTime,testFixture:true}),expiry,fixtureTime);fixtureDb.close();
  const isbn=await call('taoshu_lookup_isbn',{query:'模拟豆瓣书目'});assert.equal(isbn.cached,true);assert.equal(isbn.items[0].isbn,'9787040406641');assert.equal(isbn.selectionRequired,true);
  assert.equal((await api('/agents/isbn',{subjectId:'987'})).item.isbn,'9787040406641');
  const bookId=saved.bookId;
  await api(`/commerce/books/${bookId}/profile`,{publicationYear:'2014',volume:'上册',confirmed:true});
  const details=await api(`/commerce/books/${bookId}`);assert.equal(details.profile.agentEvidence[0].url,book.sourceUrl);
  await call('taoshu_import_school_book',{source:'smoke-agent',externalId:'school-1',sourceUrl:'https://example.org/official/catalog',bookId,school:'模拟大学',year:2027,major:'计算机',raw:'模拟图书 第5版 上册',edition:'第5版',volume:'上册'});
  await call('taoshu_import_source',{source:'smoke-agent',externalId:'source-1',sourceUrl:'https://example.org/books/1',bookId,url:'https://item.taobao.com/item.htm?id=123',title:'模拟货源',price:12.5,freight:null});
  await call('taoshu_import_image',{source:'smoke-agent',externalId:'image-1',sourceUrl:'https://example.org/cover.png',bookId,base64:png().toString('base64'),mimeType:'image/png'});
  const versions=(await call('taoshu_search_books',{query:'模拟 Agent'})).items;assert.equal(versions.length,2);assert.equal(new Set(versions.map(b=>b.edition)).size,2);
  const source=(await api(`/commerce/books/${bookId}`)).sources[0];assert.equal(source.data.status,'pending');assert.equal(source.data.freight,null);
  const secret=JSON.parse(await fs.readFile(path.join(root,'agent-support/connection.json'),'utf8')).token;
  assert.ok(!JSON.stringify(config).includes(secret));assert.ok(!stderr.includes(secret));assert.ok(!stderr.includes(token));
  const disabled=await api('/agents/mcp',{enabled:false});assert.equal(disabled.enabled,false);
  assert.ok((await rpc('tools/list',{})).error,'revocation must apply to an existing stdio bridge');
  await api('/agents/mcp',{enabled:true});
  bridge.stdin.end();await once(bridge,'exit');bridge=null;
  console.log('PASS: HTTP-backed stdio MCP initialize, tool discovery, book/school/source/image imports, idempotency, profile evidence preservation, scoped token and revocation.');
  console.log('PASS: actual MCP batch partial success/replay and cached ISBN lookup; ISBN success uses an explicitly seeded isolated fixture.');
  if(process.argv.includes('--douban')){const live=await api('/agents/isbn',{query:'三体'});console.log('LIVE_DOUBAN: '+JSON.stringify(live));}
  if(process.argv.includes('--install')){
    await api('/agents/opencode/install',{});
    let state,lastStage='';
    for(let n=0;n<1300;n++){
      state=await api('/agents/opencode');
      const job=state.install,stage=job.total?job.stage+' '+Math.floor(job.downloaded/job.total*10)*10+'%':job.stage;
      if(stage&&stage!==lastStage){console.log('INSTALL: '+stage);lastStage=stage;}
      if(job.state==='completed'||job.state==='failed')break;await wait(1000);
    }
    assert.equal(state.install.state,'completed',JSON.stringify(state.install));assert.equal(state.managed.installed,true);
    assert.equal(state.local[0].version,localBefore.local[0].version,'local OpenCode must remain unchanged');
    await api('/agents/opencode',{mode:'managed'});
    console.log('PASS: official dedicated OpenCode install, SHA-512 verification, version probe and mode selection.');
    await fs.writeFile(path.join(root,'selected-opencode.txt'),state.managed.path);
  }
  console.log('TEST_DATA: '+root);
  if(process.argv.includes('--serve')){console.log('TEST_API: '+base);await new Promise(resolve=>{process.once('SIGINT',resolve);process.once('SIGTERM',resolve);});}
}finally{
  if(bridge&&bridge.exitCode==null){bridge.stdin.end();bridge.kill();}
  if(server.exitCode==null){const stopped=once(server,'exit');server.kill();await stopped;}
  assert.equal(path.dirname(path.resolve(root)),path.resolve(os.tmpdir()));assert.ok(path.basename(root).startsWith('taoshu-agents-'));
  await fs.rm(root,{recursive:true,force:true,maxRetries:8,retryDelay:250});
}
