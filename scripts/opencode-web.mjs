// Private loopback gateway for the official OpenCode web UI. Credentials stay in this process.
import http from 'node:http';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {randomBytes} from 'node:crypto';
import {once} from 'node:events';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
const trustedFrames=["http://tauri.localhost","tauri://localhost","http://localhost:1420","http://127.0.0.1:1420"];
export async function gateway({upstream,password,frameOrigin,workspace}={}) {
  let target=upstream,origin;
  const peers=new Set(),requests=new Set();
  const host=randomBytes(24).toString('hex')+'.localhost';
  const frames=[...trustedFrames];
  if(frameOrigin){const u=new URL(frameOrigin);if(u.protocol==='http:'&&['127.0.0.1','localhost'].includes(u.hostname)&&u.origin===frameOrigin)frames.push(frameOrigin);}
  const projectPath=value=>{const resolved=path.resolve(value);return process.platform==='win32'?resolved.toLowerCase():resolved;};
  const sameProject=value=>{try{return projectPath(value)===projectPath(workspace)||projectPath(decodeURIComponent(value))===projectPath(workspace);}catch{return false;}};
  const allowed=req=>{
    if(!req.url.startsWith('/')||req.url.startsWith('//')||req.headers.host!==new URL(origin).host||(req.headers.origin&&req.headers.origin!==origin))return false;
    if(workspace){
      const u=new URL(req.url,origin);
      for(const name of ['directory','worktree'])for(const value of u.searchParams.getAll(name))if(!sameProject(value))return false;
      for(const name of ['x-opencode-directory','x-opencode-worktree'])if(req.headers[name]&&!sameProject(req.headers[name]))return false;
    }
    return true;
  };
  const requestUrl=req=>{const u=new URL(target+req.url);if(workspace&&u.pathname.startsWith('/api/'))u.searchParams.set('directory',workspace);return u;};
  const headers=req=>{const h={...req.headers,host:new URL(target).host,authorization:'Basic '+Buffer.from('opencode:'+password).toString('base64')};if(workspace)h['x-opencode-directory']=encodeURIComponent(workspace);delete h.cookie;delete h['proxy-authorization'];delete h['x-kf-token'];return h;};
  const server=http.createServer((req,res)=>{
    if(!allowed(req)){res.writeHead(403);res.end('Forbidden');return;}
    if(!target){res.writeHead(503);res.end('Starting');return;}
    const next=http.request(requestUrl(req),{method:req.method,headers:headers(req)},response=>{
      const policy=(response.headers['content-security-policy']||'').split(';').filter(value=>!value.trim().startsWith('frame-ancestors')).filter(value=>value.trim()).join('; ');
      const h={...response.headers,'referrer-policy':'no-referrer','cache-control':'no-store','content-security-policy':`${policy?policy+'; ':''}frame-ancestors ${frames.join(' ')}`};
      delete h['x-frame-options'];delete h['set-cookie'];delete h['access-control-allow-origin'];delete h['access-control-allow-credentials'];
      if(h.location?.startsWith(target))h.location=origin+h.location.slice(target.length);
      res.writeHead(response.statusCode,h);response.pipe(res);
    });
    requests.add(next);next.once('close',()=>requests.delete(next));
    next.on('error',()=>{if(!res.headersSent)res.writeHead(502);res.end();});
    req.on('aborted',()=>next.destroy());res.on('close',()=>next.destroy());req.pipe(next);
  });
  server.on('connection',socket=>{peers.add(socket);socket.on('close',()=>peers.delete(socket));});
  server.on('upgrade',(req,socket,head)=>{
    if(!allowed(req)||!target){socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');return;}
    const next=http.request(requestUrl(req),{method:req.method,headers:headers(req)});
    requests.add(next);next.once('close',()=>requests.delete(next));
    next.on('upgrade',(response,remote,tail)=>{
      peers.add(remote);remote.on('close',()=>peers.delete(remote));
      const h=Object.entries(response.headers).filter(([key])=>!['set-cookie','authorization'].includes(key)).map(([key,value])=>`${key}: ${value}`).join('\r\n');
      socket.write(`HTTP/1.1 101 Switching Protocols\r\n${h}\r\n\r\n`);
      if(tail.length)socket.write(tail);if(head.length)remote.write(head);
      socket.pipe(remote);remote.pipe(socket);socket.on('error',()=>remote.destroy());remote.on('error',()=>socket.destroy());socket.on('close',()=>remote.destroy());remote.on('close',()=>socket.destroy());
    });
    next.on('response',response=>{response.resume();socket.end('HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n');});
    next.on('error',()=>socket.destroy());socket.on('close',()=>next.destroy());next.end();
  });
  server.listen(0,'127.0.0.1');await once(server,'listening');origin=`http://${host}:${server.address().port}`;
  return {url:origin,port:server.address().port,setTarget(value){target=value;},async close(){for(const r of requests)r.destroy();for(const p of peers)p.destroy();await new Promise(resolve=>server.close(resolve));}};
}
async function availablePort(){const s=net.createServer();s.listen(0,'127.0.0.1');await once(s,'listening');const port=s.address().port;await new Promise(resolve=>s.close(resolve));return port;}
async function main(){
  const [executable,workspace,profile,parentPid]=process.argv.slice(2);
  if(!executable||!workspace)throw Error('缺少 OpenCode 程序或工作目录');
  const password=randomBytes(32).toString('hex');
  if(!path.isAbsolute(workspace))throw Error('工作目录必须是固定项目的绝对路径');
  const proxy=await gateway({password,frameOrigin:process.env.TAOSHU_WEB_ORIGIN,workspace});
  const env={...process.env,OPENCODE_PASSWORD:password,OPENCODE_SERVER_PASSWORD:password};
  if(profile){for(const [key,folder] of [['XDG_CONFIG_HOME','config'],['XDG_DATA_HOME','data'],['XDG_CACHE_HOME','cache'],['XDG_STATE_HOME','state']])env[key]=profile+'/'+folder;env.OPENCODE_DB=profile+'/data/opencode.db';}
  const port=await availablePort(),upstream=`http://127.0.0.1:${port}`;
  const child=spawn(executable,['serve','--hostname','127.0.0.1','--port',String(port),'--cors',proxy.url],{cwd:workspace,env,windowsHide:true,stdio:'ignore'});
  let stopping=false,exited=false;
  child.on('exit',()=>{exited=true;if(!stopping)shutdown(1);});child.on('error',()=>shutdown(1));
  async function shutdown(code=0){if(stopping)return;stopping=true;clearInterval(parentWatch);if(!exited)child.kill();await proxy.close();process.exit(code);}
  const parentWatch=setInterval(()=>{try{process.kill(Number(parentPid),0);}catch{shutdown();}},2000);
  process.stdin.resume();process.stdin.on('end',()=>shutdown());process.on('SIGTERM',()=>shutdown());process.on('SIGINT',()=>shutdown());
  for(let attempt=0;attempt<100&&!exited;attempt++){
    try{const response=await fetch(upstream+'/api/info',{headers:{authorization:'Basic '+Buffer.from('opencode:'+password).toString('base64')},signal:AbortSignal.timeout(1000)});if(response.ok){proxy.setTarget(upstream);process.stdout.write(JSON.stringify({url:proxy.url})+'\n');return;}}catch{}
    await new Promise(resolve=>setTimeout(resolve,200));
  }
  await shutdown(1);
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href)main().catch(()=>{process.stdout.write(JSON.stringify({error:'OpenCode 网页服务启动失败，请确认所选版本可运行'})+'\n');process.exitCode=1;});
