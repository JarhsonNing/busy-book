import zlib from 'node:zlib';
import fs from 'node:fs';
import path from 'node:path';

const SIZE = 1024;
const pixels = Buffer.alloc(SIZE * SIZE * 4);

function put(x, y, r, g, b, a = 255) {
  if (x < 0 || y < 0 || x >= SIZE || y >= SIZE) return;
  const offset = (y * SIZE + x) * 4;
  pixels[offset] = r;
  pixels[offset + 1] = g;
  pixels[offset + 2] = b;
  pixels[offset + 3] = a;
}

function blend(x, y, r, g, b, alpha) {
  if (x < 0 || y < 0 || x >= SIZE || y >= SIZE) return;
  const offset = (y * SIZE + x) * 4;
  const a = alpha / 255;
  pixels[offset] = Math.round(pixels[offset] * (1 - a) + r * a);
  pixels[offset + 1] = Math.round(pixels[offset + 1] * (1 - a) + g * a);
  pixels[offset + 2] = Math.round(pixels[offset + 2] * (1 - a) + b * a);
  pixels[offset + 3] = 255;
}

function roundedRect(x0, y0, x1, y1, radius, color) {
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) {
      const dx = Math.max(x0 + radius - x, 0, x - (x1 - radius - 1));
      const dy = Math.max(y0 + radius - y, 0, y - (y1 - radius - 1));
      if (dx * dx + dy * dy <= radius * radius) {
        blend(x, y, color[0], color[1], color[2], color[3] ?? 255);
      }
    }
  }
}

// 背景：深墨色圆角方块
roundedRect(32, 32, 992, 992, 210, [42, 34, 26, 255]);

// 内圈细边
for (let angle = 0; angle < Math.PI * 2; angle += 0.0004) {
  const x = 512 + Math.cos(angle) * 448;
  const y = 512 + Math.sin(angle) * 448;
  blend(Math.round(x), Math.round(y), 122, 96, 66, 110);
}

// 书脊
roundedRect(258, 252, 330, 772, 26, [122, 70, 44, 255]);
// 封面
roundedRect(330, 252, 782, 772, 30, [243, 231, 209, 255]);
// 封面暗边
roundedRect(330, 252, 782, 772, 30, [0, 0, 0, 0]);
for (let y = 252; y < 772; y += 1) {
  for (let x = 330; x < 782; x += 1) {
    const edge = x > 764 || y > 754 || x < 346 || y < 268;
    if (edge) blend(x, y, 205, 185, 156, 90);
  }
}

// 封面上的横线（书名装饰）
for (let line = 0; line < 4; line += 1) {
  const top = 350 + line * 78;
  roundedRect(400, top, 400 + (line === 2 ? 220 : 300), top + 26, 13, [70, 55, 42, 235]);
}

// 朱砂印章
const circle = (cx, cy, radius, color) => {
  for (let y = cy - radius; y <= cy + radius; y += 1) {
    for (let x = cx - radius; x <= cx + radius; x += 1) {
      const dx = x - cx;
      const dy = y - cy;
      if (dx * dx + dy * dy <= radius * radius) blend(x, y, color[0], color[1], color[2], color[3]);
    }
  }
};
circle(686, 668, 66, [176, 66, 45, 255]);
circle(686, 668, 52, [243, 231, 209, 255]);
circle(686, 668, 44, [176, 66, 45, 255]);
circle(686, 668, 22, [243, 231, 209, 255]);

// 写 PNG
const raw = Buffer.alloc((SIZE * 4 + 1) * SIZE);
for (let y = 0; y < SIZE; y += 1) {
  raw[y * (SIZE * 4 + 1)] = 0;
  pixels.copy(raw, y * (SIZE * 4 + 1) + 1, y * SIZE * 4, (y + 1) * SIZE * 4);
}

const chunks = [];
function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body) >>> 0, 0);
  chunks.push(Buffer.concat([length, body, crc]));
}

function crc32(buffer) {
  let crc = 0xffffffff;
  for (const byte of buffer) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

const header = Buffer.alloc(13);
header.writeUInt32BE(SIZE, 0);
header.writeUInt32BE(SIZE, 4);
header[8] = 8;
header[9] = 6;
header[10] = 0;
header[11] = 0;
header[12] = 0;

chunk('IHDR', header);
chunk('IDAT', zlib.deflateSync(raw, { level: 9 }));
chunk('IEND', Buffer.alloc(0));

const png = Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), ...chunks]);
const target = path.resolve('src-tauri/icon-source.png');
fs.writeFileSync(target, png);
console.log(`icon written: ${target} (${png.length} bytes)`);
