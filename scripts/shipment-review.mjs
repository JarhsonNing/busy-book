// Frontend acceptance with simulated orders only. Never contacts a marketplace.
import http from 'node:http';
import {createServer} from 'vite';
const port=1423, origin=`http://127.0.0.1:${port}`;
const requests=[], states=new Map();
for(const id of [1,2]) {
  const snapshot={order_no:`MOCK-SALE-${id}`,order_status:12,refund_status:0,pay_amount:3800,order_time:1793145600,seller_name:'模拟店铺',goods:{title:`模拟订单 · 图书 ${id}`,quantity:1}};
  states.set(id,{id,orderNo:snapshot.order_no,kind:'unknown',state:'unmatched',bookId:null,snapshot,data:{},purchases:[],events:[],deliveries:[]});
}
const vite=await createServer({server:{middlewareMode:true,watch:{ignored:['**/src-tauri/**','**/dist/**']}},appType:'custom'});
const html=`<!doctype html><meta charset="utf-8"><title>发货流程 · 模拟验收</title><div id="app"></div><script type="module">
import {createApp,h} from 'vue';
import Workbench from '/src/components/XianyuWorkbench.vue';import {initApi} from '/src/api.js';
import {modalFocus} from '/src/modal-focus.js';import Button from '/src/components/ui/Button.vue';import Select from '/src/components/ui/SearchSelect.vue';
import '/src/styles.css';import '/src/ui.css';import '/src/redesign.css';
await initApi();createApp({render:()=>h('main',{class:'studio-content'},[h('p',{class:'muted',style:'margin-bottom:16px'},'模拟订单 · 本地验收'),h(Workbench)])}).directive('modal-focus',modalFocus).component('UiButton',Button).component('SearchSelect',Select).mount('#app');
</script>`;
function order(f){return {...f.snapshot,fulfillment:{id:f.id,state:f.state,kind:f.kind,purchaseState:f.purchases.find(p=>p.state!=='canceled')?.state},receiver_name:'模拟收件人',receiver_mobile:'模拟电话',address:'模拟地址'};}
async function body(req){let s='';for await(const c of req)s+=c;return s?JSON.parse(s):{};}
http.createServer(async(req,res)=>{
  const url=new URL(req.url,origin), path=url.pathname;
  const send=(data,status=200)=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(data));};
  if(path==='/src/api.js'){
    let source=(await vite.transformRequest('/src/api.js')).code;
    source=source.replace("http://127.0.0.1:8899",origin).replace(/export async function openBrowser\(kind, url\) \{[\s\S]*?\n\}/,"export async function openBrowser(kind,url){await fetch('/fixture/open',{method:'POST',body:JSON.stringify({kind,url})});}");
    res.writeHead(200,{'content-type':'application/javascript'});res.end(source);return;
  }
  if(path.startsWith('/fixture/')){
    if(path==='/fixture/audit'){send(requests);return;}
    requests.push({path,body:await body(req)});send({ok:true});return;
  }
  if(path.startsWith('/api/')){
    const input=await body(req);requests.push({path,body:input});
    if(path==='/api/token'){send({token:'mock-local-token',baseUrl:origin});return;}
    if(path==='/api/xianyu/config'){send({secretConfigured:true});return;}
    if(path==='/api/xianyu/call/shops'){send({list:[{authorize_id:'1',user_name:'模拟店铺'}]});return;}
    if(path==='/api/xianyu/call/companies'){send({list:[{express_code:'SF',express_name:'顺丰'},{express_code:'YTO',express_name:'圆通'}]});return;}
    if(path==='/api/xianyu/call/orders'){const list=[...states.values()].filter(f=>!input.order_status||f.snapshot.order_status===input.order_status).map(order);send({list,count:list.length});return;}
    if(path==='/api/xianyu/call/order'){send(order([...states.values()].find(f=>f.orderNo===input.order_no)));return;}
    if(path==='/api/commerce/fulfillments'){send([...states.values()].filter(f=>!url.searchParams.has('orderNo')||f.orderNo===url.searchParams.get('orderNo')));return;}
    const match=path.match(/^\/api\/commerce\/fulfillments\/(\d+)(?:\/(\w+))?$/);
    if(match){
      const f=states.get(Number(match[1])),action=match[2];
      if(!action){send(f);return;}
      if(action==='tracking') {f.kind='physical';f.data.shipping={...input,origin:'manual'};f.state='ready_to_ship';}
      if(action==='purchase') {
        f.kind='physical';const purchase={id:1,state:input.state,data:{...input,source:{id:null,data:{}}}};f.purchases=[purchase];
        f.state=input.state==='shipped'?'ready_to_ship':input.state==='canceled'?'to_purchase':'awaiting_tracking';
        if(input.state==='shipped')f.data.shipping={trackingNo:input.trackingNo,expressCode:input.expressCode,expressName:input.expressName,origin:'purchase'};
      }
      if(action==='ship') {
        if(!input.confirmed){send({error:'请确认物流'},400);return;}
        requests.push({path:'SIMULATED-REMOTE-SHIP',body:{order_no:f.orderNo,waybill_no:f.data.shipping.trackingNo,express_code:f.data.shipping.expressCode,express_name:f.data.shipping.expressName}});
        f.data.accepted=true;f.state='shipping_unknown';f.data.shippingUncertain=true;send({status:'accepted'});return;
      }
      if(action==='refresh' && f.data.accepted){f.snapshot.order_status=21;f.state='shipped';f.data.shippingUncertain=false;}
      send({ok:true});return;
    }
    if(path==='/api/commerce/library'){send({items:[],total:0});return;}
    send({error:'Unhandled fixture route: '+path},404);return;
  }
  if(path==='/'){res.writeHead(200,{'content-type':'text/html;charset=utf-8'});res.end(await vite.transformIndexHtml('/',html));return;}
  vite.middlewares(req,res);
}).listen(port,'127.0.0.1',()=>console.log('Simulated shipment review: '+origin));
