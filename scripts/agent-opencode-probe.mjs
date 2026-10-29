// Actual OpenCode interoperability without model inference or the user's shared service.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {randomBytes} from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import {once} from 'node:events';
const [root,bin]=process.argv.slice(2);
assert.ok(root&&bin,'Usage: node scripts/agent-opencode-probe.mjs TEST_DATA OPENCODE_EXE');
const profile=path.join(root,'client-probe'),password=randomBytes(24).toString('hex');
const env={...process.env,OPENCODE_DB:path.join(profile,'opencode.db'),OPENCODE_SERVER_PASSWORD:password};
for(const [key,folder] of [['XDG_CONFIG_HOME','config'],['XDG_DATA_HOME','data'],['XDG_CACHE_HOME','cache'],['XDG_STATE_HOME','state']]){env[key]=path.join(profile,folder);await fs.mkdir(env[key],{recursive:true});}
const child=spawn(bin,['serve','--hostname','127.0.0.1','--port','18998'],{env,cwd:path.join(root,'agent-support/workspace'),windowsHide:true,stdio:'ignore'});
try{
  let connected=false;
  // Location plugins initialize asynchronously; one-shot CLI listing can return an empty catalog.
  for(let attempt=0;attempt<30;attempt++){
    await new Promise(resolve=>setTimeout(resolve,1000));
    try{const response=await fetch('http://127.0.0.1:18998/api/mcp',{headers:{authorization:'Basic '+Buffer.from('opencode:'+password).toString('base64')}});const result=await response.json();connected=result.data?.some(item=>item.name==='taoshu'&&item.status.status==='connected');if(connected)break;}catch{}
  }
  assert.equal(connected,true,'OpenCode did not connect to the generated MCP configuration');
  const response=await fetch('http://127.0.0.1:18998/api/skill',{headers:{authorization:'Basic '+Buffer.from('opencode:'+password).toString('base64')}});
  const skills=await response.json();for(const name of ['taoshu-import','taoshu-workflow'])assert.ok(skills.data?.some(skill=>skill.name===name),'OpenCode did not discover '+name+' Skill');
  console.log('PASS: actual OpenCode MCP connected; import and workflow Skills discovered; no model inference.');
}finally{if(child.exitCode===null){const stopped=once(child,'exit');child.kill();await stopped;}}
