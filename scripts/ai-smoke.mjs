// Real OpenCode CLI + isolated application + explicitly simulated local model.
// --serve keeps the fixture available for UI review; no paid/free provider is called.
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {DatabaseSync} from 'node:sqlite';
const executable=process.argv[2];assert.ok(executable,'Pass the verified OpenCode v2 executable');
const port=process.env.TAOSHU_TEST_PORT || '18997';
const parent=path.resolve('.redesign-test-data');await fs.mkdir(parent,{recursive:true});
const root=await fs.mkdtemp(path.join(parent,'ai-system-'));
const answer={title:'模拟 AI 图书上架草稿',description:'模拟模型生成的图书描述，仅用于本地功能验收。',sellPrice:1,referenceSourceId:null,analysis:'模拟分析：按已核验规格、采购价及运费比较到手成本，实际售价由系统校验利润底线。',text:'模拟自定义推广文本（无模型推理）'};
let mode='normal',calls=0,held=[],lastBody;
const model=http.createServer(async(req,res)=>{
  if(req.method==='GET'){res.setHeader('content-type','application/json');res.end(JSON.stringify({data:[{id:'book-writer',object:'model',owned_by:'fixture'}]}));return;}
  const chunks=[];for await(const c of req)chunks.push(c);
  lastBody=JSON.parse(Buffer.concat(chunks).toString());calls++;
  const reply=()=>{
    if(res.destroyed)return;
    if(lastBody.stream){
      res.writeHead(200,{'content-type':'text/event-stream'});
      const chunk=(delta,finish_reason)=>({id:'chatcmpl_fixture',object:'chat.completion.chunk',created:Math.floor(Date.now()/1000),model:'book-writer',choices:[{index:0,delta,finish_reason}]});
      res.write('data: '+JSON.stringify(chunk({role:'assistant',content:JSON.stringify(answer)},null))+'\n\n');
      res.write('data: '+JSON.stringify({...chunk({},'stop'),usage:{prompt_tokens:100,completion_tokens:100,total_tokens:200}})+'\n\n');res.end('data: [DONE]\n\n');
    }else{res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({id:'chatcmpl_fixture',object:'chat.completion',created:Math.floor(Date.now()/1000),model:'book-writer',choices:[{index:0,message:{role:'assistant',content:JSON.stringify(answer)},finish_reason:'stop'}],usage:{prompt_tokens:100,completion_tokens:100,total_tokens:200}}));}
  };
  if(mode==='hold')held.push(reply);else reply();
});
model.listen(0,'127.0.0.1');await once(model,'listening');
await fs.writeFile(path.join(root,'opencode.json'),JSON.stringify({$schema:'https://opencode.ai/config.json',providers:{fixture:{name:'模拟本地模型（验收）',env:[],package:'@opencode/ai/providers/openai-compatible',settings:{baseURL:'http://127.0.0.1:'+model.address().port+'/v1',apiKey:'test-fixture-only'},models:{'book-writer':{name:'模拟图书文案模型'}}}}}));
const env={...process.env,PORT:port,TAOSHU_DATA:root,TAOSHU_WEB_ORIGIN:'http://127.0.0.1:1424',OPENCODE_DB:path.join(root,'profile/data/opencode.db')};
for(const [key,folder] of [['XDG_CONFIG_HOME','config'],['XDG_DATA_HOME','data'],['XDG_CACHE_HOME','cache'],['XDG_STATE_HOME','state']]){env[key]=path.join(root,'profile',folder);await fs.mkdir(env[key],{recursive:true});}
const server=spawn(path.resolve(process.env.TAOSHU_TEST_API_BINARY || 'src-tauri/target/debug/api-server.exe'),[],{env,windowsHide:true,stdio:'ignore'});
let token;
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function api(route,method='GET',body){const r=await fetch('http://127.0.0.1:'+port+'/api'+route,{method,headers:{'x-kf-token':token||'','content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const data=await r.json();assert.ok(r.ok,JSON.stringify(data));return data;}
async function until(fn,label,seconds=50){for(let n=0;n<seconds*5;n++){const result=await fn();if(result)return result;await wait(200);}throw Error('Timed out: '+label);}
async function finished(id){return until(async()=>{const j=await api('/commerce/jobs/'+id);return ['completed','failed','partial','canceled'].includes(j.state)?j:null;},'AI job '+id);}
let sourceNo=10000;
async function source(bookId,price=10,freight=2){return api('/commerce/sources','POST',{bookId,primary:false,data:{url:'https://item.taobao.com/item.htm?id='+(++sourceNo),title:'模拟已核验货源',sku:'第5版',condition:'九成新',price,freight,status:'available',dropship:true,checkedAt:'2026-10-29'}});}
try{
  await until(async()=>{assert.equal(server.exitCode,null,'test API exited before startup');try{const r=await fetch('http://127.0.0.1:'+port+'/api/token');if(r.ok){token=(await r.json()).token;return true;}}catch{}},'API startup',15);
  await api('/agents/opencode','POST',{mode:'local',path:path.resolve(executable)});
  const config=await api('/agents/ai/config');config.model='fixture/book-writer';await api('/agents/ai/config','POST',config);
  const book=await api('/books','POST',{title:'模拟 · 数据库系统概论',author:'模拟作者',isbn:'9787040406641',edition:'第5版',costPrice:8,sellPrice:20,stock:0});
  const sid=(await source(book.id)).id;
  await source(book.id,1,null);
  const job=await api('/commerce/jobs','POST',{kind:'ai',bookIds:[book.id],task:'complete'});
  const result=await finished(job.id);
  if(result.state!=='completed')console.log('FAILED_FIXTURE: '+root+'; '+JSON.stringify(result));
  assert.equal(result.state,'completed',JSON.stringify(result));
  const draft=await api('/agents/ai/books/'+book.id+'/draft');assert.equal(draft.draft.title,answer.title);assert.equal(draft.draft.costPrice,10);assert.equal(draft.draft.sellPrice,17);assert.equal(draft.draft.stale,false);
  const saved=await api('/books/'+book.id);assert.equal(saved.costPrice,10);assert.equal(saved.sellPrice,17);assert.equal(saved.stock,0);
  assert.ok(lastBody.messages.some(m=>JSON.stringify(m).includes('模拟 · 数据库系统概论')),'book context was not attached');
  const exposed=(lastBody.tools||[]).map(t=>t.function?.name||t.name||t.type);
  assert.ok(exposed.every(name=>name==='skill'),'text agent must not receive shell/file/MCP/browser/subagent tools');
  assert.ok(lastBody.messages.some(m=>m.role==='system' && String(m.content).includes('成本固定取淘宝有效商品价')),'restricted draft agent was not selected');
  assert.equal((await api('/commerce/books/'+book.id)).sources.find(s=>s.id===sid).primary,false);
  const preview=await api('/xianyu/books/'+book.id+'/preview');assert.equal(preview.title,answer.title);assert.equal(preview.description,answer.description);assert.equal(preview.price,17);
  // MCP-scoped command line creates a task, waits, and prints generated text.
  await api('/agents/mcp','POST',{enabled:true});
  const cli=spawn(process.execPath,[path.join(root,'agent-support/ai-generate.mjs'),'--book',String(book.id),'--task','text','--text'],{windowsHide:true,stdio:['ignore','pipe','pipe']});
  let output='',error='';cli.stdout.on('data',c=>output+=c);cli.stderr.on('data',c=>error+=c);const [code]=await once(cli,'exit');assert.equal(code,0,error);assert.match(output,/模拟自定义推广文本/);
  const before=(await api('/agents/ai/books/'+book.id+'/draft')).draft.revision;
  mode='hold';const baseline=calls;const interrupted=await api('/commerce/jobs','POST',{kind:'ai',bookIds:[book.id],task:'pricing'});
  await until(()=>calls>baseline,'held model request');await api('/commerce/jobs/'+interrupted.id+'/pause','POST',{});
  await wait(700);assert.equal((await api('/agents/ai/books/'+book.id+'/draft')).draft.revision,before);
  mode='normal';for(const release of held.splice(0))release();await api('/commerce/jobs/'+interrupted.id+'/resume','POST',{});assert.equal((await finished(interrupted.id)).state,'completed');
  // A manual edit while inference is running must win over the old snapshot.
  mode='hold';const baselineEdit=calls;const editing=await api('/commerce/jobs','POST',{kind:'ai',bookIds:[book.id],task:'complete'});
  await until(()=>calls>baselineEdit,'concurrent edit request');
  await api('/books/'+book.id,'PATCH',{sellPrice:31});mode='normal';for(const release of held.splice(0))release();
  const conflict=await finished(editing.id);assert.equal(conflict.state,'failed');assert.match(conflict.result.items[0].error,/修改/);assert.equal((await api('/books/'+book.id)).sellPrice,31);
  // Automatic source changes are debounced; neither them nor draft writes select a source.
  config.enabled=true;config.autoOnImport=false;config.autoOnSources=true;await api('/agents/ai/config','POST',config);
  await api('/commerce/sources','POST',{id:sid,bookId:book.id,primary:false,data:{url:'https://item.taobao.com/item.htm?id=10001',title:'模拟已核验货源',sku:'第5版',condition:'九成新',price:11,freight:3,status:'available',dropship:true,checkedAt:'2026-10-29'}});
  const auto=await until(async()=>{const j=(await api('/commerce/jobs')).find(j=>j.kind==='ai'&&j.request.auto&&j.request.bookIds.includes(book.id));return j;},'automatic source task');assert.equal((await finished(auto.id)).state,'completed');assert.equal((await api('/books/'+book.id)).costPrice,11);
  const unknown=await api('/books','POST',{title:'模拟 · 运费待核实图书',edition:'第5版',costPrice:6,sellPrice:18,stock:0});await source(unknown.id,1,null);
  const partial=await api('/commerce/jobs','POST',{kind:'ai',bookIds:[unknown.id,book.id],task:'complete'});assert.equal((await finished(partial.id)).state,'completed');assert.equal((await api('/books/'+unknown.id)).costPrice,null);assert.equal((await api('/books/'+unknown.id)).sellPrice,18);
  const db=new DatabaseSync(path.join(root,'shelf.db'));assert.equal(db.prepare('SELECT count(*) n FROM xianyu_tasks').get().n,0);assert.equal(db.prepare('SELECT count(*) n FROM channel_products').get().n,0);assert.equal(db.prepare('SELECT count(*) n FROM book_sources WHERE is_primary=1').get().n,0);db.close();
  console.log('PASS: real OpenCode CLI + simulated local model, evidence context, tool denial, pricing floor, automatic drafts, preview reuse, scoped MCP CLI text, pause/resume, concurrent-edit preservation, automatic source trigger and unknown freight. No remote publication/model billing.');
  if(process.argv.includes('--serve')){console.log('TEST_API: http://127.0.0.1:18997; TEST_DATA: '+root);await new Promise(resolve=>{process.once('SIGINT',resolve);process.once('SIGTERM',resolve);process.stdin.once('data',resolve);process.stdin.resume();});process.stdin.pause();}
}finally{
  if(server.exitCode==null){const stopped=once(server,'exit');server.kill();await stopped;}
  model.closeAllConnections();await new Promise(resolve=>model.close(resolve));
  // Preserve a failed fixture for diagnostics; successful fixture cleanup is explicit.
  if(process.argv.includes('--clean')){assert.equal(path.dirname(root),parent);await fs.rm(root,{recursive:true,force:true,maxRetries:8,retryDelay:250});}
}
