// Local DOM acceptance using sanitized user-provided markup. No marketplace requests.
import http from 'node:http';
import fs from 'node:fs/promises';
const source = await fs.readFile(new URL('../src-tauri/inject/browser.js',import.meta.url),'utf8');
const extract = source.slice(source.indexOf('  function extractTaobaoSearchCandidates('),source.indexOf('  async function collectTaobaoSearch()'));
const fixture = await fs.readFile(new URL('./fixtures/taobao-cards.html',import.meta.url),'utf8');
const html = `<!doctype html><meta charset="utf-8"><title>淘宝结构本地验证</title>
<style>body{font:16px system-ui;padding:24px}a{display:block;color:inherit;text-decoration:none;margin:12px}.scrollWrapper--GN04JPE5{height:120px;overflow-y:auto;border:1px solid #ddd}.priceInt--yqqZMJ5a,.priceFloat--XpixvyQ1{display:inline}pre{white-space:pre-wrap}</style>
<h1>用户提供结构 · 本地验证</h1><pre id="result">运行中</pre>${fixture}
<script>const sourceBookId=17,lookupParams=new URLSearchParams('q=fixture');${extract}
try {
 const items=extractTaobaoSearchCandidates();
 if(JSON.stringify(items.map(i=>i.price))!=='[52.48,19,22.48]') throw Error('价格不符');
 if(JSON.stringify(items.map(i=>i.shop))!==JSON.stringify(['启诚图书专营店','中南书店','小谷吖书店'])) throw Error('店铺不符');
 if(items[0].priceLabel!=='补贴后'||items[1].priceLabel!=='') throw Error('价格标签不符');
 const root=document.querySelector('[class*="scrollWrapper"]');
 scrollTaobaoResults();
 if(root.scrollTop<=0||root.scrollTop>root.clientHeight) throw Error('结果容器没有按可视高度滚动');
 const pager=document.createElement('div');pager.className='next-pagination next-simple';pager.innerHTML='<button class="next-next">轮播下一页</button>';document.body.append(pager);
 if(taobaoNextButton()) throw Error('误选轮播分页');
 pager.className='next-pagination next-normal';
 if(!taobaoNextButton()) throw Error('未找到真实结果分页');
 pager.querySelector('button').disabled=true;
 if(taobaoNextButton()) throw Error('误选禁用分页');
 document.querySelector('#result').textContent='PASS: 三张真实结构卡片、整数/小数价格、补贴标签、容器滚动与分页辨别\u005cn'+JSON.stringify(items.map(({price,priceLabel,shop})=>({price,priceLabel,shop})),null,2);
}catch(e){document.querySelector('#result').textContent='FAIL: '+e.message;}
</script>`;
http.createServer((req,res)=>{res.writeHead(200,{'content-type':'text/html;charset=utf-8','content-security-policy':"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'"});res.end(html);}).listen(18996,'127.0.0.1',()=>console.log('Local DOM fixture: http://127.0.0.1:18996'));
