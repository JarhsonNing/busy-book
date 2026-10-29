// Scoped MCP client: starts draft generation and prints the saved result. No app token.
import fs from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const args=process.argv.slice(2);
const option=name=>{const n=args.indexOf(name);return n<0?undefined:args[n+1];};
if(args.includes('--help')){
  console.log('node ai-generate.mjs --book 1,2 --task complete|pricing|copy|text|sources [--provider opencode|qoder] [--prompt "提示词"] [--model MODEL] [--text] [--no-wait] [--config connection.json]');
  process.exit(0);
}
try{
  const ids=[...new Set((option('--book')||'').split(',').map(Number))];
  if(!ids.length || ids.length>100 || ids.some(id=>!Number.isSafeInteger(id)||id<=0))throw Error('通过 --book 指定 1–100 个有效图书编号');
  const task=option('--task')||'complete';
  if(!['complete','pricing','copy','text','sources'].includes(task))throw Error('未知任务类型');
  const config=JSON.parse(await fs.readFile(option('--config')||path.join(path.dirname(fileURLToPath(import.meta.url)),'connection.json'),'utf8'));
  const url=new URL(config.url);
  if(url.protocol!=='http:' || !['localhost','127.0.0.1'].includes(url.hostname) || url.pathname!=='/mcp')throw Error('接入配置须指向本地淘书工坊 MCP');
  let id=0;
  async function call(input){
    const res=await fetch(url,{method:'POST',headers:{authorization:'Bearer '+config.token,'content-type':'application/json'},body:JSON.stringify({jsonrpc:'2.0',id:++id,method:'tools/call',params:{name:'taoshu_generate_draft',arguments:input}}),signal:AbortSignal.timeout(15000)});
    const reply=await res.json();if(!res.ok || reply.error || reply.result?.isError)throw Error(reply.error?.message||reply.error||reply.result?.content?.[0]?.text||'MCP 不可用，请检查应用与导入设置');
    return reply.result.structuredContent;
  }
  const input={action:'start',bookIds:ids,task};
  for(const name of ['provider','prompt','model'])if(option('--'+name)!==undefined)input[name]=option('--'+name);
  let job=await call(input);
  if(args.includes('--no-wait')){console.log(JSON.stringify(job));process.exit(0);}
  while(['queued','running','paused'].includes(job.state)){
    if(job.state==='paused')throw Error('任务已暂停，请在采集页继续任务');
    await new Promise(resolve=>setTimeout(resolve,1000));job=await call({action:'status',jobId:job.id});
  }
  if(args.includes('--text')){
    for(const item of job.result?.items||[]){
      if(ids.length>1)console.log(item.title);
      if(item.state==='done')console.log(task==='text'?item.draft.text:['pricing','sources'].includes(task)?item.draft.analysis:[item.draft.title,item.draft.description].filter(Boolean).join('\n\n'));
      else console.error(item.error||'生成失败');
    }
  }else console.log(JSON.stringify(job,null,2));
  if(job.state!=='completed')process.exitCode=1;
}catch(error){console.error(error.message);process.exitCode=1;}
