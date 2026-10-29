import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import net from 'node:net';
import {once} from 'node:events';
import {gateway} from './opencode-web.mjs';
async function setup(handler,workspace){const upstream=http.createServer(handler);const connections=new Set();upstream.on('connection',s=>{connections.add(s);s.on('close',()=>connections.delete(s));});upstream.listen(0,'127.0.0.1');await once(upstream,'listening');const base=`http://127.0.0.1:${upstream.address().port}`;const proxy=await gateway({upstream:base,password:'test-secret',frameOrigin:'http://127.0.0.1:1424',workspace});const host=new URL(proxy.url).host;return {upstream,base,proxy,host,async close(){await proxy.close();for(const s of connections)s.destroy();upstream.closeAllConnections();await new Promise(r=>upstream.close(r));}};}
async function request(fixture,path='/',headers={}){return new Promise((resolve,reject)=>{const req=http.get({hostname:'127.0.0.1',port:fixture.proxy.port,path,headers:{host:fixture.host,...headers}},res=>{const chunks=[];res.on('data',c=>chunks.push(c));res.on('end',()=>resolve({status:res.statusCode,headers:res.headers,body:Buffer.concat(chunks).toString()}));});req.on('error',reject);});}

test('gateway pins API requests to the fixed project and refuses directory switches',async()=>{
  const workspace=process.cwd();let count=0,received;
  const f=await setup((req,res)=>{count++;received=req;res.end('ok');},workspace);
  try{
    for(const route of ['/api/session?directory=..','/api/session?directory='+encodeURIComponent(workspace)+'&directory=..','/api/session?worktree=..'])assert.equal((await request(f,route)).status,403);
    assert.equal((await request(f,'/api/session',{'x-opencode-directory':'..'})).status,403);
    assert.equal(count,0);
    assert.equal((await request(f,'/api/session')).status,200);
    assert.equal(new URL(received.url,'http://local').searchParams.get('directory'),workspace);
    assert.equal(received.headers['x-opencode-directory'],encodeURIComponent(workspace));
    assert.equal((await request(f,'/api/session?directory='+encodeURIComponent(workspace))).status,200);
  }finally{await f.close();}
});
test('gateway authenticates upstream only and rejects other hosts and origins',async()=>{
  let received,count=0;const f=await setup((req,res)=>{count++;received=req.headers;res.setHeader('content-security-policy',"default-src 'self'; frame-ancestors 'none'");res.setHeader('set-cookie','private=value');res.end('ok');});
  try{
    assert.equal((await request(f,'/',{host:'127.0.0.1:'+f.proxy.port})).status,403);
    assert.equal((await request(f,'/',{origin:'https://evil.example'})).status,403);
    assert.equal((await request(f,'http://evil.example/')).status,403);assert.equal(count,0);
    const result=await request(f,'/api/info',{cookie:'other=secret','x-kf-token':'app-secret',authorization:'untrusted'});
    assert.equal(result.status,200);assert.equal(received.authorization,'Basic '+Buffer.from('opencode:test-secret').toString('base64'));assert.equal(received.cookie,undefined);assert.equal(received['x-kf-token'],undefined);
    assert.equal(result.headers['set-cookie'],undefined);assert.equal(result.headers['referrer-policy'],'no-referrer');assert.ok(result.headers['content-security-policy'].includes("default-src 'self'"));assert.ok(result.headers['content-security-policy'].includes('frame-ancestors http://tauri.localhost'));assert.ok(!JSON.stringify(result).includes('test-secret'));
  }finally{await f.close();}
});
test('gateway forwards streaming responses without waiting for completion',async()=>{
  let finish;const f=await setup((req,res)=>{res.writeHead(200,{'content-type':'text/event-stream'});res.write('data: ready\n\n');finish=()=>res.end('data: done\n\n');});
  try{await new Promise((resolve,reject)=>{const req=http.get({hostname:'127.0.0.1',port:f.proxy.port,path:'/api/event',headers:{host:f.host}},res=>{res.once('data',chunk=>{assert.match(chunk.toString(),/ready/);finish();resolve();});});req.on('error',reject);});}finally{await f.close();}
});
test('gateway tunnels authenticated WebSocket upgrades and raw bytes',async()=>{
  const f=await setup((req,res)=>res.end());
  f.upstream.on('upgrade',(req,socket)=>{assert.equal(req.headers.authorization,'Basic '+Buffer.from('opencode:test-secret').toString('base64'));socket.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n');socket.on('data',chunk=>socket.write(chunk));});
  const socket=net.connect(f.proxy.port,'127.0.0.1');
  try{await once(socket,'connect');socket.write(`GET /ws HTTP/1.1\r\nHost: ${f.host}\r\nOrigin: ${f.proxy.url}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`);const [head]=await once(socket,'data');assert.match(head.toString(),/101 Switching/);socket.write('echo');const [echo]=await once(socket,'data');assert.equal(echo.toString(),'echo');}finally{socket.destroy();await f.close();}
});
