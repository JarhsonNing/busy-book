// UI acceptance against the isolated agent-smoke API; never connects to a marketplace.
import http from 'node:http';
import {createServer} from 'vite';
const reviewPort=Number(process.env.TAOSHU_REVIEW_PORT || 1424);
const origin='http://127.0.0.1:'+reviewPort, backend=process.env.TAOSHU_REVIEW_BACKEND || 'http://127.0.0.1:18997';
const vite=await createServer({server:{middlewareMode:true,watch:{ignored:['**/src-tauri/**','**/dist/**']}},appType:'custom'});
const html=`<!doctype html><meta charset="utf-8"><title>Agent 接入 · 本地验收</title><div id="app"></div><script type="module">
import {createApp} from 'vue';import App from '/src/App.vue';
import {modalFocus} from '/src/modal-focus.js';import Button from '/src/components/ui/Button.vue';import Select from '/src/components/ui/SearchSelect.vue';
import '/src/styles.css';import '/src/ui.css';import '/src/redesign.css';
createApp(App).directive('modal-focus',modalFocus).component('UiButton',Button).component('SearchSelect',Select).mount('#app');
</script>`;
const server=http.createServer(async(req,res)=>{
  try{
    const route=new URL(req.url,origin).pathname;
    if(route==='/src/api.js'){
      const source=(await vite.transformRequest('/src/api.js')).code.replace('http://127.0.0.1:8899',origin).replace('if (data.baseUrl) BASE = data.baseUrl;',`BASE = '${origin}';`);
      res.writeHead(200,{'content-type':'application/javascript'});res.end(source);return;
    }
    if(route.startsWith('/api/')){
      const chunks=[];for await(const chunk of req)chunks.push(chunk);
      const headers={...req.headers};delete headers.origin;delete headers.host;delete headers['content-length'];
      const upstream=await fetch(backend+req.url,{method:req.method,headers,body:['GET','HEAD'].includes(req.method)?undefined:Buffer.concat(chunks)});
      res.writeHead(upstream.status,{'content-type':upstream.headers.get('content-type')||'application/json'});res.end(Buffer.from(await upstream.arrayBuffer()));return;
    }
    if(route==='/'){res.writeHead(200,{'content-type':'text/html;charset=utf-8'});res.end(await vite.transformIndexHtml('/',html));return;}
    vite.middlewares(req,res);
  }catch(e){res.writeHead(500);res.end(e.message);}
});
server.listen(reviewPort,'127.0.0.1',()=>console.log('Agent settings review: '+origin));
process.on('SIGINT',async()=>{server.close();await vite.close();process.exit(0);});
