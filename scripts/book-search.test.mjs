import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import vm from 'node:vm';

// Execute the real injected search handler with a simulated website response.
const source = await fs.readFile(new URL('../src-tauri/inject/browser.js', import.meta.url), 'utf8');
const start = source.indexOf('  async function searchBookMetadata()');
const end = source.indexOf('  /* --------------------------- 新窗口', start);
assert.ok(start > 0 && end > start);
async function run(response) {
  const sent = [], fetched = [];
  const context = {
    bookLookup: { id: 'requirement-1', requestId: 'request-2', query: '数据库 第5版' },
    state: { busy: false }, panelSub: {}, URLSearchParams, AbortSignal,
    setBusy() {}, openPanel() {}, toast() {}, refreshSiteButton() {},
    async fetch(url, options) { fetched.push({url, options}); return {ok: true, json: async () => response}; },
    async requestJson(url, options) { sent.push({url, ...options}); return { items: options.body.items || [] }; },
  };
  await vm.runInNewContext(source.slice(start, end) + '\nsearchBookMetadata()', context);
  return {sent, fetched};
}
test('website search returns metadata, source and distinct versions without creating books', async () => {
  const {sent, fetched} = await run({status:1,data:{itemResponse:{list:[
    {itemName:'<b>数据库</b> 第5版', author:'王珊',press:'高教',isbn:'9787040406641',shopId:'10',itemId:'20'},
    {itemName:'数据库 第6版',link:{pc:'https://book.kongfz.com/10/21/'}},
  ]}}});
  assert.equal(fetched[0].options.credentials,'same-origin');
  assert.ok(fetched[0].url.includes('/product/keyword/list?'));
  assert.equal(sent.length,1);
  assert.equal(sent[0].body.items[0].edition,'第5版');
  assert.equal(sent[0].body.items[1].edition,'第6版');
  assert.equal(sent[0].body.items[0].sourceUrl,'https://book.kongfz.com/10/20/');
  assert.equal(sent[0].body.requestId,'request-2');
});
test('login rejection and schema changes are errors, not empty search results', async () => {
  for (const data of [{status:0,message:'请登录后再进行访问。'}, {status:1,data:{}}]) {
    const {sent} = await run(data);
    assert.equal(sent.length,1);
    assert.ok(sent[0].body.error);
    assert.equal(sent[0].body.items,undefined);
  }
});
