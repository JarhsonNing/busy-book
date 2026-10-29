// Extract exactly the official executable to a fresh candidate; never unpack paths from the archive.
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {spawn} from 'node:child_process';
import {pipeline} from 'node:stream/promises';
import {Transform} from 'node:stream';
import {once} from 'node:events';
const [archive,target,expected]=process.argv.slice(2);
try {
  const hash=createHash('sha512');for await(const chunk of fs.createReadStream(archive))hash.update(chunk);
  if(hash.digest('base64')!==expected)throw Error('安装包 SHA-512 校验失败，请重新安装');
  const child=spawn('tar.exe',['-xOf',archive,'package/bin/opencode.exe'],{windowsHide:true,stdio:['ignore','pipe','pipe']});
  const exited=once(child,'exit');exited.catch(()=>{});let stderr='',size=0;
  child.stderr.on('data',chunk=>stderr=(stderr+chunk).slice(0,1000));
  const timeout=setTimeout(()=>child.kill(),50000);
  try {
    await pipeline(child.stdout,new Transform({transform(chunk,encoding,done){size+=chunk.length;done(size>500*1024*1024?Error('程序超过大小限制'):null,chunk);}}),fs.createWriteStream(target,{flags:'wx'}));
    const [code]=await exited;if(code!==0||size===0)throw Error('无法解压官方程序：'+stderr);
  } catch(error){child.kill();throw error;} finally {clearTimeout(timeout);}
  console.log('verified');
} catch(error){console.error(error.message);process.exitCode=1;}
