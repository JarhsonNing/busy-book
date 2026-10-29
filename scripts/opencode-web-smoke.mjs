// Real headless OpenCode acceptance in an isolated profile, without model inference.
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {once} from 'node:events';
const executable=process.argv[2];assert.ok(executable,'Pass the verified OpenCode executable');
const root=await fs.mkdtemp(path.join(os.tmpdir(),'taoshu-web-'));
const env={...process.env,PORT:'18996',TAOSHU_DATA:root,OPENCODE_DB:path.join(root,'profile/opencode.db')};
for(const [key,folder] of [['XDG_CONFIG_HOME','config'],['XDG_DATA_HOME','data'],['XDG_CACHE_HOME','cache'],['XDG_STATE_HOME','state']]){env[key]=path.join(root,'profile',folder);await fs.mkdir(env[key],{recursive:true});}
const server=spawn(process.env.TAOSHU_TEST_API_BINARY || 'src-tauri/target/debug/api-server.exe',[],{env,windowsHide:true,stdio:'ignore'});
const wait=ms=>new Promise(r=>setTimeout(r,ms));let token;
async function api(route,method='GET',body){const r=await fetch('http://127.0.0.1:18996/api/agents/opencode'+route,{method,headers:{'x-kf-token':token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const value=await r.json();assert.ok(r.ok,JSON.stringify(value));return value;}
async function web(url,route='/',method='GET',body){const origin=new URL(url);return new Promise((resolve,reject)=>{const r=http.request({hostname:'127.0.0.1',port:origin.port,path:route,method,headers:{host:origin.host,origin:origin.origin,'content-type':'application/json'}},response=>{let text='';response.on('data',chunk=>text+=chunk);response.on('end',()=>resolve({status:response.statusCode,text}));});r.on('error',reject);r.end(body===undefined?undefined:JSON.stringify(body));});}
async function children(pid){const result=await promisify(execFile)('powershell.exe',['-NoProfile','-NonInteractive','-Command',`@(Get-CimInstance Win32_Process -Filter 'ParentProcessId=${Number(pid)}' | Select-Object ProcessId,Name) | ConvertTo-Json -Compress`],{windowsHide:true});return [].concat(JSON.parse(result.stdout||'[]'));}
try{
  for(let i=0;i<100;i++){await wait(100);assert.equal(server.exitCode,null,'acceptance API exited');try{const r=await fetch('http://127.0.0.1:18996/api/token');if(r.ok){token=(await r.json()).token;break;}}catch{}}
  assert.ok(token);assert.equal((await api('/web')).running,false);
  assert.equal((await fetch('http://127.0.0.1:18996/api/agents/opencode/web')).status,401);
  const enable=await fetch('http://127.0.0.1:18996/api/agents/mcp',{method:'POST',headers:{'x-kf-token':token,'content-type':'application/json'},body:JSON.stringify({enabled:true})});assert.ok(enable.ok);
  await api('','POST',{mode:'local',path:path.resolve(executable)});
  const first=await api('/launch','POST',{});assert.equal(first.running,true);assert.ok(!JSON.stringify(first).includes('password'));
  assert.equal((await api('/launch','POST',{})).url,first.url);
  assert.equal((await web(first.url)).status,200);assert.match((await web(first.url)).text,/<title[^>]*>OpenCode/);
  const info=await web(first.url,'/api/info');assert.equal(info.status,200);
  const created=await web(first.url,'/api/session','POST',{});assert.equal(created.status,200,created.text);const saved=JSON.parse(created.text);const id=saved.data?.id||saved.id;assert.ok(id,created.text);
  const refused=await fetch('http://127.0.0.1:18996/api/agents/opencode',{method:'POST',headers:{'x-kf-token':token,'content-type':'application/json'},body:JSON.stringify({mode:'managed'})});assert.equal(refused.status,400);
  await api('/web','DELETE');assert.equal((await api('/web')).running,false);await assert.rejects(()=>web(first.url));
  const second=await api('/launch','POST',{});assert.notEqual(second.url,first.url);assert.equal((await web(second.url,'/api/session/'+id)).status,200,'session must survive restart');
  const proxies=await children(server.pid),descendants=[...proxies];for(const proxy of proxies)descendants.push(...await children(proxy.ProcessId));
  assert.ok(descendants.some(p=>/opencode/i.test(p.Name)),'private OpenCode process must be present');
  const stopped=once(server,'exit');server.kill();await stopped;await wait(500);
  await assert.rejects(()=>web(second.url));for(const p of descendants){assert.throws(()=>process.kill(p.ProcessId,0),'owned process survived application exit: '+p.Name);}
  console.log('PASS: hidden web start/reuse, protected app API, official UI, session persistence, guarded mode switch, explicit stop, and Windows process-tree cleanup. No model inference.');
}finally{if(server.exitCode===null&&server.signalCode===null){const stopped=once(server,'exit');server.kill();await stopped;}}
