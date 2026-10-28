import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const exec=promisify(execFile);
test('catalog build separates editions and retains stable IDs across rebuilds',async()=>{
 const root=await fs.mkdtemp(path.join(os.tmpdir(),'taoshu-catalog-editions-'));
 try{
  const input=path.join(root,'input'),out=path.join(root,'out');await fs.mkdir(input);
  await fs.writeFile(path.join(input,'school.json'),JSON.stringify({school:'测试大学',year:2027,collectedAt:'2026-10-26',source:{adapter:'article',entryUrl:'https://example.org/catalog'},programs:[{college:'数学学院',majorCode:'0701',majorName:'数学',pageUrl:'https://example.org/catalog',subjects:[{stage:'初试',subjectName:'数学分析',subjectCode:'601',books:[2,3].map(n=>({title:'数学分析',authors:'甲',publisher:'高等教育出版社',edition:'第'+n+'版',volume:'上册',year:'2020',raw:'数学分析第'+n+'版上册'}))}]}]}));
  const build=async()=>{await exec(process.execPath,['scripts/kaoyan/build-catalog.mjs','--input',input,'--out',out],{windowsHide:true});return JSON.parse(await fs.readFile(path.join(out,'catalog.json'),'utf8'));};
  const first=await build(),second=await build();
  assert.equal(first.books.length,2);assert.equal(first.requirements.length,2);assert.deepEqual(first.books.map(b=>b.id),second.books.map(b=>b.id));assert.deepEqual(first.requirements.map(r=>r.id),second.requirements.map(r=>r.id));
  assert.equal(new Set(first.books.map(b=>b.edition)).size,2);
 }finally{await fs.rm(root,{recursive:true,force:true});}
});
