// Simulated Qoder CLI + real application queue. No remote model or user credentials.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
const parent=path.resolve('.redesign-test-data');await fs.mkdir(parent,{recursive:true});
const root=await fs.mkdtemp(path.join(parent,"qoder ai 中文 '-"));
const port=process.env.TAOSHU_TEST_PORT || '19041',base='http://127.0.0.1:'+port;
const answer={title:'模拟 Qoder 上架草稿',description:'本地模拟 Qoder 生成的描述',sellPrice:1,analysis:'模拟分析：根据已知图书和货源证据定价，运费及版次需要核实。',text:'模拟 Qoder 自定义文本'};
const mock=path.join(root,'mock-qoder.mjs'),program=path.join(root,'qoder.cmd');
await fs.writeFile(mock,`
import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
const root=path.dirname(fileURLToPath(import.meta.url)),args=process.argv.slice(2),file=name=>path.join(root,name);
if(args.includes('--version')){console.log('Qoder 1.2.3 fixture');process.exit(0);}
if(args.includes('status')){console.log(JSON.stringify({logged_in:fs.existsSync(file('logged-in')),email:'private-fixture@example.test',token:'fixture-secret'}));process.exit(0);}
if(args.includes('login')){fs.writeFileSync(file('logged-in'),'true');fs.writeFileSync(file('login-cwd'),process.cwd());await new Promise(r=>setTimeout(r,500));process.exit(0);}
if(args.includes('--list-models')){if(!fs.existsSync(file('logged-in'))){console.error('Not logged in. fixture-secret');process.exit(1);}console.log('MODEL\\nFixture-Qoder\\nAuto\\nCustom fixture (byok-fixture)');process.exit(0);}
if(args.includes('--print')){
  let prompt='';for await(const chunk of process.stdin)prompt+=chunk;
  fs.writeFileSync(file('last-call.json'),JSON.stringify({cwd:process.cwd(),args,prompt}));
  while(fs.existsSync(file('hold')))await new Promise(r=>setTimeout(r,100));
  if(fs.existsSync(file('error'))){console.log(JSON.stringify({type:'result',subtype:'error_during_execution',is_error:true,errors:['quota 429 fixture-secret']}));process.exit(0);}
  console.log(JSON.stringify({type:'result',subtype:'success',is_error:false,result:JSON.stringify(${JSON.stringify(answer)})}));process.exit(0);
}
fs.writeFileSync(file('terminal-call.json'),JSON.stringify({cwd:process.cwd(),args}));await new Promise(r=>setTimeout(r,30000));
`);
await fs.writeFile(program,`@echo off\r\nchcp 65001 >nul\r\n"${process.execPath}" "${mock}" %*\r\n`);
const server=spawn(path.resolve(process.env.TAOSHU_TEST_API_BINARY || 'src-tauri/target/debug/api-server.exe'),[],{env:{...process.env,PORT:port,TAOSHU_DATA:root,QODER_CONFIG_DIR:path.join(root,'profile')},windowsHide:true,stdio:'ignore'});
let token;const wait=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label,seconds=35){for(let i=0;i<seconds*5;i++){const value=await fn();if(value)return value;await wait(200);}throw Error('Timed out: '+label);}
async function api(route,method='GET',body){const res=await fetch(base+'/api'+route,{method,headers:{'x-kf-token':token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const value=await res.json();assert.ok(res.ok,JSON.stringify(value));return value;}
async function finished(id){return until(async()=>{const job=await api('/commerce/jobs/'+id);return ['completed','failed','partial'].includes(job.state)?job:null;},'job '+id);}
async function lastCall(){try{return JSON.parse(await fs.readFile(path.join(root,'last-call.json'),'utf8'));}catch{return null;}}
try{
  await until(async()=>{assert.equal(server.exitCode,null,'test API exited');try{const r=await fetch(base+'/api/token');if(r.ok){token=(await r.json()).token;return true;}}catch{}},'API startup');
  await api('/agents/qoder','POST',{mode:'local',path:program});
  assert.deepEqual(await api('/agents/qoder/account'),{loggedIn:false});
  const denied=await fetch(base+'/api/agents/ai/models?provider=qoder',{headers:{'x-kf-token':token}});const deniedText=await denied.text();assert.equal(denied.status,400);assert.match(deniedText,/登录/);assert.ok(!deniedText.includes('fixture-secret'));
  // Login is independent of MCP enablement, and still uses the fixed project.
  await api('/agents/qoder/login','POST',{});
  await until(async()=>!(await api('/agents/qoder/cli')).running,'login console exit');
  assert.deepEqual(await api('/agents/qoder/account'),{loggedIn:true});
  assert.equal(await fs.readFile(path.join(root,'login-cwd'),'utf8'),path.join(root,'agent-support/workspace'));
  assert.deepEqual((await api('/agents/ai/models?provider=qoder')).items,['Fixture-Qoder','Auto','byok-fixture']);
  let config=await api('/agents/ai/config');config.model='fixture/writer';config=await api('/agents/ai/config','POST',config);
  config.provider='qoder';config.model='Fixture-Qoder';config=await api('/agents/ai/config','POST',config);
  assert.equal(config.models.opencode,'fixture/writer');assert.equal(config.models.qoder,'Fixture-Qoder');
  const book=await api('/books','POST',{title:'模拟 · Qoder 图书',edition:'第5版',sellPrice:20,stock:0});
  for(const [i,price] of [1,10].entries())await api('/commerce/sources','POST',{bookId:book.id,primary:false,data:{url:'https://item.taobao.com/item.htm?id='+(10001+i),title:'模拟报价',sku:'第5版',condition:'九成新',price,freight:2,status:'available',dropship:true,checkedAt:'2026-10-29'}});
  const complete=await api('/commerce/jobs','POST',{kind:'ai',bookIds:[book.id],task:'complete'});const completed=await finished(complete.id);assert.equal(completed.state,'completed',JSON.stringify(completed));
  const draft=(await api('/agents/ai/books/'+book.id+'/draft')).draft;assert.equal(draft.provider,'qoder');assert.equal(draft.model,'Fixture-Qoder');assert.equal(draft.title,answer.title);assert.equal(draft.sellPrice,17);
  const call=await lastCall();assert.equal(call.cwd,path.join(root,'agent-support/workspace'));assert.ok(call.args.includes('--strict-mcp-config'));assert.equal(call.args[call.args.indexOf('--tools')+1],'');assert.equal(call.args[call.args.indexOf('--permission-mode')+1],'dont_ask');assert.equal(JSON.parse(call.args[call.args.indexOf('--settings')+1]).disableAllHooks,true);assert.match(call.prompt,/模拟 · Qoder 图书/);
  // A long prompt goes through stdin instead of Windows command-line arguments.
  const long=await api('/commerce/jobs','POST',{kind:'ai',bookIds:[book.id],task:'text',prompt:'x'.repeat(11000)});assert.equal((await finished(long.id)).state,'completed');assert.ok((await lastCall()).prompt.length>11000);
  await fs.writeFile(path.join(root,'hold'),'');
  const held=await api('/commerce/jobs','POST',{kind:'ai',bookIds:[book.id],task:'text',prompt:'pause-and-resume'});
  await until(async()=> (await lastCall())?.prompt.includes('pause-and-resume'),'held Qoder prompt');
  await api('/commerce/jobs/'+held.id+'/pause','POST',{});
  config.provider='opencode';config.model='fixture/writer';config=await api('/agents/ai/config','POST',config);
  await fs.unlink(path.join(root,'hold'));await api('/commerce/jobs/'+held.id+'/resume','POST',{});
  const resumed=await finished(held.id);assert.equal(resumed.state,'completed',JSON.stringify(resumed));assert.equal(resumed.request.provider,'qoder');assert.equal(resumed.request.model,'Fixture-Qoder');
  const revision=(await api('/agents/ai/books/'+book.id+'/draft')).draft.revision;
  await fs.writeFile(path.join(root,'error'),'');
  const broken=await api('/commerce/jobs','POST',{kind:'ai',bookIds:[book.id],provider:'qoder',task:'text'});const failed=await finished(broken.id);assert.equal(failed.state,'failed');assert.match(failed.result.items[0].error,/额度/);assert.ok(!JSON.stringify(failed).includes('fixture-secret'));assert.equal((await api('/agents/ai/books/'+book.id+'/draft')).draft.revision,revision);
  await fs.unlink(path.join(root,'error'));await api('/commerce/jobs/'+broken.id+'/retry','POST',{});assert.equal((await finished(broken.id)).state,'completed');
  const command=await api('/agents/ai/command','POST',{bookId:book.id,provider:'qoder',model:'Auto',task:'text',prompt:'copy-this-prompt'});assert.match(command.command,/--provider 'qoder' --model 'Auto'/);
  const connection=await api('/agents/mcp','POST',{enabled:true});
  const cli=spawn(process.execPath,[path.join(root,'agent-support/ai-generate.mjs'),'--book',String(book.id),'--task','text','--provider','qoder','--model','Auto','--text'],{windowsHide:true,stdio:['ignore','pipe','pipe']});let output='',error='';cli.stdout.on('data',c=>output+=c);cli.stderr.on('data',c=>error+=c);const [code]=await once(cli,'exit');assert.equal(code,0,error);assert.match(output,/模拟 Qoder 自定义文本/);assert.equal((await api('/agents/ai/books/'+book.id+'/draft')).draft.model,'Auto');
  await api('/agents/qoder/launch','POST',{});
  const terminal=await until(async()=>{try{return JSON.parse(await fs.readFile(path.join(root,'terminal-call.json'),'utf8'));}catch{}},'terminal model');
  assert.equal(terminal.args[terminal.args.indexOf('--model')+1],'Fixture-Qoder');assert.equal(terminal.cwd,connection.configs.workspace);await api('/agents/qoder/cli','DELETE');
  console.log('PASS: Qoder login/account privacy, native-format model list, per-executor model memory, real queue/draft pricing, stdin prompts, no-tool flags, pause/resume and retry preserve Qoder after changing defaults, failed output is not applied, scoped MCP CLI provider/model. Simulated CLI; no remote inference.');
  console.log('TEST_DATA: '+root);
  if(process.argv.includes('--serve')){console.log('TEST_API: '+base);await new Promise(resolve=>{process.once('SIGINT',resolve);process.once('SIGTERM',resolve);process.stdin.once('data',resolve);process.stdin.resume();});}
}finally{if(server.exitCode===null&&server.signalCode===null){const exit=once(server,'exit');server.kill();await exit;}}
