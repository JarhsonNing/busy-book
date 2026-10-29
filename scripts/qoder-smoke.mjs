// Isolated native-install and terminal acceptance. Never sends a model prompt.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {once} from 'node:events';
const parent=path.resolve('.redesign-test-data');await fs.mkdir(parent,{recursive:true});
const root=await fs.mkdtemp(path.join(parent,"qoder verify 中文 '-"));
const port=process.env.TAOSHU_TEST_PORT || '19038',base='http://127.0.0.1:'+port;
const server=spawn(path.resolve(process.env.TAOSHU_TEST_API_BINARY || 'src-tauri/target/debug/api-server.exe'),[],{env:{...process.env,PORT:port,TAOSHU_DATA:root},windowsHide:true,stdio:'ignore'});
let token;const wait=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label,seconds=30){for(let i=0;i<seconds*5;i++){const result=await fn();if(result)return result;await wait(200);}throw Error('Timed out: '+label);}
async function api(route,method='GET',body){const res=await fetch(base+'/api/agents'+route,{method,headers:{'x-kf-token':token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const value=await res.json();assert.ok(res.ok,JSON.stringify(value));return value;}
try{
  await until(async()=>{assert.equal(server.exitCode,null,'test API exited');try{const res=await fetch(base+'/api/token');if(res.ok){token=(await res.json()).token;return true;}}catch{}},'API startup');
  assert.equal((await fetch(base+'/api/agents/qoder')).status,401);
  const mcp=await api('/mcp','POST',{enabled:true});const workspace=mcp.configs.workspace;
  assert.equal(workspace,path.join(root,'agent-support/workspace'));
  for(const skill of ['taoshu-import','taoshu-workflow'])await fs.access(path.join(workspace,'.qoder/skills',skill,'SKILL.md'));
  const before=await api('/qoder');assert.equal(before.workspace,workspace);
  if(process.argv.includes('--install')){
    await api('/qoder','POST',{mode:'managed'});await api('/qoder/install','POST',{});
    const installed=await until(async()=>{const v=await api('/qoder');return v.install.state!=='running'?v:null;},'native download',600);
    assert.equal(installed.install.state,'completed',JSON.stringify(installed));assert.equal(installed.managed.installed,true);
    const env={...process.env,QODER_CONFIG_DIR:path.join(root,'isolated-profile')};
    const run=promisify(execFile);
    const version=await run(installed.selectedPath,['--version'],{cwd:workspace,env,windowsHide:true,timeout:15000});
    assert.match(version.stdout,/\d+\.\d+\.\d+/);
    const help=await run(installed.selectedPath,['--help'],{cwd:workspace,env,windowsHide:true,timeout:15000});
    assert.match(help.stdout,/--cwd/);assert.match(help.stdout,/--mcp-config/);assert.match(help.stdout,/--strict-mcp-config/);
    console.log('PASS: official Qoder native package, SHA-256 extraction, installed version '+version.stdout.trim()+', CLI cwd/MCP flags.');
  }
  const mock=path.join(root,'qoder.cmd');
  await fs.writeFile(mock,'@echo off\r\nchcp 65001 >nul\r\nif "%~1"=="--version" (echo Qoder 1.2.3& exit /b 0)\r\necho %CD%>"%~dp0cwd.txt"\r\necho %*>"%~dp0args.txt"\r\nping -n 120 127.0.0.1 >nul\r\n');
  await api('/qoder','POST',{mode:'local',path:mock});
  assert.equal((await api('/qoder/launch','POST',{workspace:parent,cwd:parent})).running,true);
  await until(async()=>{try{return (await fs.readFile(path.join(root,'cwd.txt'),'utf8')).trim();}catch{}},'terminal startup');
  assert.equal((await fs.readFile(path.join(root,'cwd.txt'),'utf8')).trim(),workspace);
  const args=await fs.readFile(path.join(root,'args.txt'),'utf8');assert.match(args,/--cwd/);assert.match(args,/--strict-mcp-config/);assert.ok(args.includes(workspace));
  assert.equal((await api('/qoder/launch','POST',{})).running,true);
  const refused=await fetch(base+'/api/agents/qoder',{method:'POST',headers:{'x-kf-token':token,'content-type':'application/json'},body:JSON.stringify({mode:'managed'})});assert.equal(refused.status,400);
  assert.equal((await api('/qoder/cli','DELETE')).running,false);
  assert.equal((await api('/qoder/cli')).running,false);
  // Restart, then prove closing the application owns the entire console tree.
  await api('/qoder/launch','POST',{});
  const run=promisify(execFile);
  const children=async pid=>{const r=await run('powershell.exe',['-NoProfile','-NonInteractive','-Command',`@(Get-CimInstance Win32_Process -Filter 'ParentProcessId=${Number(pid)}' | Select-Object -ExpandProperty ProcessId) | ConvertTo-Json -Compress`],{windowsHide:true});return [].concat(JSON.parse(r.stdout||'[]'));};
  const owned=await children(server.pid);for(let i=0;i<owned.length;i++)owned.push(...await children(owned[i]));assert.ok(owned.length>=2);
  const exited=once(server,'exit');server.kill();await exited;await wait(500);for(const pid of owned)assert.throws(()=>process.kill(pid,0),'Qoder descendant survived app exit');
  console.log('PASS: protected API, shared project and Skills, Unicode/spaces/apostrophe paths, ignored external cwd, terminal launch/reuse, guarded selection, explicit stop and process-tree cleanup.');
  console.log('TEST_DATA: '+root);
}finally{if(server.exitCode===null&&server.signalCode===null){const exited=once(server,'exit');server.kill();await exited;}}
