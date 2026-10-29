// Standard newline-delimited MCP bridge. Only MCP messages go to stdout.
import fs from 'node:fs/promises';
import readline from 'node:readline';
const configIndex=process.argv.indexOf('--config');
if(configIndex<0 || !process.argv[configIndex+1]) { process.stderr.write('Missing --config connection file\n');process.exit(1); }
const configPath=process.argv[configIndex+1];
let protocol='2025-11-25';
const lines=readline.createInterface({input:process.stdin,crlfDelay:Infinity});
for await(const line of lines) {
  let message;
  try {
    if(line.length>16*1024*1024) throw Error('MCP request is too large');
    if(!line.trim()) continue;
    message=JSON.parse(line);
    const config=JSON.parse(await fs.readFile(configPath,'utf8'));
    const endpoint=new URL(config.url);
    if(endpoint.protocol!=='http:' || !['127.0.0.1','localhost','[::1]'].includes(endpoint.hostname) || endpoint.pathname!=='/mcp') throw Error('Only the local Taoshu MCP endpoint is supported');
    const response=await fetch(endpoint,{method:'POST',headers:{'content-type':'application/json',accept:'application/json, text/event-stream',authorization:'Bearer '+config.token,'MCP-Protocol-Version':protocol},body:JSON.stringify(message),signal:AbortSignal.timeout(message.method==='tools/call'&&message.params?.name==='taoshu_lookup_isbn'?60000:30000)});
    if(response.status===202 || response.status===204) continue;
    const result=await response.json();
    if(!response.ok) throw Error(result.error?.message || result.error || `Local MCP HTTP ${response.status}`);
    if(message.method==='initialize' && result.result?.protocolVersion) protocol=result.result.protocolVersion;
    if(Object.hasOwn(message,'id')) process.stdout.write(JSON.stringify(result)+'\n');
  } catch(error) {
    if(message && Object.hasOwn(message,'id')) process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:message.id,error:{code:-32000,message:error.message}})+'\n');
    else if(!message) process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:null,error:{code:-32700,message:error.message}})+'\n');
    else process.stderr.write('Taoshu MCP: '+error.message+'\n');
  }
}
