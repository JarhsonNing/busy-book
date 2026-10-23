import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const source = path.join(root, 'src-tauri', 'inject', 'browser.js');
const targetDir = path.join(root, 'extension', 'content');
const target = path.join(targetDir, 'inject.js');

if (!fs.existsSync(source)) {
  console.error('找不到注入脚本：', source);
  process.exit(1);
}

const config = {
  apiBase: 'http://127.0.0.1:8899',
  host: 'extension',
};
const script = fs.readFileSync(source, 'utf8');
fs.mkdirSync(targetDir, { recursive: true });
fs.writeFileSync(
  target,
  `/* 由 scripts/build-extension.mjs 自动生成，请勿直接编辑。 */\nwindow.__TAOSHU_CONFIG__ = ${JSON.stringify(config)};\n${script}`,
  'utf8',
);

console.log('已生成', target);
console.log('在 Chrome 打开 chrome://extensions → 开启开发者模式 → 加载已解压的扩展程序 → 选择 extension 目录。');
