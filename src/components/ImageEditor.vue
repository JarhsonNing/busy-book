<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { assetUrl } from '../api';

const props = defineProps({
  image: { type: Object, required: true },
});
const emit = defineEmits(['close', 'saved']);

const canvasRef = ref(null);
const loading = ref(true);
const busy = ref(false);
const status = ref('');
const tool = ref('brush');
const brushSize = ref(44);
const showMask = ref(true);
const band = ref(38);
const threshold = ref(14);
const enhance = ref(true);
const maskCount = ref(0);
const crop = reactive({ top: 0, bottom: 0, left: 0, right: 0 });

let work = null;
let workCtx = null;
let maskCanvas = null;
let maskCtx = null;
let mask = null;
let width = 0;
let height = 0;
let history = [];
let painting = false;
let lastPoint = null;

const cropRect = computed(() => {
  const x = Math.round((width * crop.left) / 100);
  const y = Math.round((height * crop.top) / 100);
  const w = Math.max(8, Math.round((width * (100 - crop.left - crop.right)) / 100));
  const h = Math.max(8, Math.round((height * (100 - crop.top - crop.bottom)) / 100));
  return { x, y, w: Math.min(w, width - x), h: Math.min(h, height - y) };
});

onMounted(load);
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey);
});

function onKey(event) {
  if (event.key === 'Escape') emit('close');
}

async function load() {
  window.addEventListener('keydown', onKey);
  const image = new Image();
  image.crossOrigin = 'anonymous';
  const source = props.image.processedUrl || props.image.url;
  try {
    await new Promise((resolve, reject) => {
      image.onload = resolve;
      image.onerror = () => reject(new Error('图片加载失败'));
      image.src = assetUrl(source);
    });
  } catch (error) {
    status.value = error.message;
    loading.value = false;
    return;
  }

  const max = 1400;
  const scale = Math.min(1, max / Math.max(image.naturalWidth, image.naturalHeight));
  width = Math.max(1, Math.round(image.naturalWidth * scale));
  height = Math.max(1, Math.round(image.naturalHeight * scale));

  work = document.createElement('canvas');
  work.width = width;
  work.height = height;
  workCtx = work.getContext('2d', { willReadFrequently: true });
  workCtx.drawImage(image, 0, 0, width, height);

  maskCanvas = document.createElement('canvas');
  maskCanvas.width = width;
  maskCanvas.height = height;
  maskCtx = maskCanvas.getContext('2d', { willReadFrequently: true });
  mask = new Uint8Array(width * height);

  const canvas = canvasRef.value;
  canvas.width = width;
  canvas.height = height;

  loading.value = false;
  await nextTick();
  draw();
}

function context() {
  return canvasRef.value.getContext('2d');
}

function draw() {
  if (!work || !canvasRef.value) return;
  const ctx = context();
  ctx.clearRect(0, 0, width, height);
  ctx.drawImage(work, 0, 0);
  if (showMask.value) ctx.drawImage(maskCanvas, 0, 0);

  const rect = cropRect.value;
  ctx.save();
  ctx.fillStyle = 'rgba(26, 20, 14, 0.52)';
  ctx.fillRect(0, 0, width, rect.y);
  ctx.fillRect(0, rect.y + rect.h, width, height - (rect.y + rect.h));
  ctx.fillRect(0, rect.y, rect.x, rect.h);
  ctx.fillRect(rect.x + rect.w, rect.y, width - (rect.x + rect.w), rect.h);
  ctx.strokeStyle = '#f3b562';
  ctx.lineWidth = Math.max(2, width / 480);
  ctx.setLineDash([14, 9]);
  ctx.strokeRect(rect.x, rect.y, rect.w, rect.h);
  ctx.restore();
}

function pointOf(event) {
  const canvas = canvasRef.value;
  const box = canvas.getBoundingClientRect();
  return {
    x: ((event.clientX - box.left) / box.width) * width,
    y: ((event.clientY - box.top) / box.height) * height,
  };
}

function pointerDown(event) {
  if (loading.value || busy.value) return;
  painting = true;
  canvasRef.value.setPointerCapture(event.pointerId);
  pushHistory();
  const point = pointOf(event);
  lastPoint = point;
  stamp(point.x, point.y);
  draw();
}

function pointerMove(event) {
  if (!painting) return;
  const point = pointOf(event);
  const from = lastPoint || point;
  const distance = Math.hypot(point.x - from.x, point.y - from.y);
  const step = Math.max(2, brushSize.value / 5);
  const steps = Math.max(1, Math.ceil(distance / step));
  for (let index = 0; index <= steps; index += 1) {
    const ratio = index / steps;
    stamp(from.x + (point.x - from.x) * ratio, from.y + (point.y - from.y) * ratio);
  }
  lastPoint = point;
  draw();
}

function pointerUp() {
  if (!painting) return;
  painting = false;
  lastPoint = null;
  countMask();
}

function stamp(x, y) {
  const radius = brushSize.value / 2;
  const erasing = tool.value === 'erase';
  maskCtx.globalCompositeOperation = erasing ? 'destination-out' : 'source-over';
  if (erasing) {
    maskCtx.fillStyle = 'rgba(0,0,0,1)';
  } else {
    maskCtx.fillStyle = 'rgba(233, 84, 61, 0.62)';
  }
  maskCtx.beginPath();
  maskCtx.arc(x, y, radius, 0, Math.PI * 2);
  maskCtx.fill();

  const left = Math.max(0, Math.floor(x - radius));
  const right = Math.min(width - 1, Math.ceil(x + radius));
  const top = Math.max(0, Math.floor(y - radius));
  const bottom = Math.min(height - 1, Math.ceil(y + radius));
  const radiusSquared = radius * radius;
  const value = erasing ? 0 : 1;
  for (let py = top; py <= bottom; py += 1) {
    for (let px = left; px <= right; px += 1) {
      const dx = px - x;
      const dy = py - y;
      if (dx * dx + dy * dy <= radiusSquared) {
        mask[py * width + px] = value;
      }
    }
  }
}

function pushHistory() {
  history.push(new Uint8Array(mask));
  if (history.length > 6) history.shift();
}

function undo() {
  const previous = history.pop();
  if (!previous) return;
  mask.set(previous);
  rebuildMaskCanvas();
  draw();
  countMask();
}

function resetAll() {
  mask.fill(0);
  history = [];
  crop.top = 0;
  crop.bottom = 0;
  crop.left = 0;
  crop.right = 0;
  enhance.value = true;
  rebuildMaskCanvas();
  draw();
  countMask();
}

function countMask() {
  let count = 0;
  for (let index = 0; index < mask.length; index += 1) {
    if (mask[index]) count += 1;
  }
  maskCount.value = count;
}

function rebuildMaskCanvas() {
  const imageData = maskCtx.createImageData(width, height);
  const data = imageData.data;
  for (let index = 0; index < mask.length; index += 1) {
    if (!mask[index]) continue;
    const offset = index * 4;
    data[offset] = 233;
    data[offset + 1] = 84;
    data[offset + 2] = 61;
    data[offset + 3] = 158;
  }
  maskCtx.globalCompositeOperation = 'source-over';
  maskCtx.putImageData(imageData, 0, 0);
}

async function detect() {
  if (loading.value || busy.value) return;
  busy.value = true;
  status.value = '正在识别半透明水印…';
  await new Promise((resolve) => setTimeout(resolve, 40));
  try {
    pushHistory();
    const startY = Math.floor(height * (1 - band.value / 100));
    const blurred = document.createElement('canvas');
    blurred.width = width;
    blurred.height = height;
    const blurredCtx = blurred.getContext('2d', { willReadFrequently: true });
    blurredCtx.filter = 'blur(9px)';
    blurredCtx.drawImage(work, 0, 0);

    const source = workCtx.getImageData(0, 0, width, height).data;
    const soft = blurredCtx.getImageData(0, 0, width, height).data;
    const detected = new Uint8Array(width * height);

    for (let y = startY; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const index = y * width + x;
        const offset = index * 4;
        const r = source[offset];
        const g = source[offset + 1];
        const b = source[offset + 2];
        const gray = 0.299 * r + 0.587 * g + 0.114 * b;
        const background = 0.299 * soft[offset] + 0.587 * soft[offset + 1] + 0.114 * soft[offset + 2];
        const saturation = Math.max(r, g, b) - Math.min(r, g, b);
        if (gray - background > threshold.value && gray > 92 && saturation < 72) {
          detected[index] = 1;
        }
      }
    }

    for (let pass = 0; pass < 2; pass += 1) {
      const previous = detected.slice();
      for (let y = startY; y < height; y += 1) {
        for (let x = 0; x < width; x += 1) {
          const index = y * width + x;
          if (previous[index]) continue;
          if (
            (x > 0 && previous[index - 1]) ||
            (x < width - 1 && previous[index + 1]) ||
            (y > 0 && previous[index - width]) ||
            (y < height - 1 && previous[index + width])
          ) {
            detected[index] = 1;
          }
        }
      }
    }

    mask.set(detected);
    rebuildMaskCanvas();
    draw();
    countMask();
    status.value = maskCount.value
      ? `已标记 ${maskCount.value} 个像素，可继续用画笔补涂`
      : '没有识别到明显水印，可手动涂抹';
  } finally {
    busy.value = false;
  }
}

async function save() {
  if (busy.value) return;
  busy.value = true;
  status.value = '正在修补并导出…';
  await new Promise((resolve) => setTimeout(resolve, 40));
  try {
    const imageData = workCtx.getImageData(0, 0, width, height);
    if (maskCount.value > 0) {
      inpaint(imageData.data, width, height, mask);
    }

    const temp = document.createElement('canvas');
    temp.width = width;
    temp.height = height;
    temp.getContext('2d').putImageData(imageData, 0, 0);

    const rect = cropRect.value;
    const scale = Math.min(1, 1200 / Math.max(rect.w, rect.h));
    const outputWidth = Math.max(1, Math.round(rect.w * scale));
    const outputHeight = Math.max(1, Math.round(rect.h * scale));

    const output = document.createElement('canvas');
    output.width = outputWidth;
    output.height = outputHeight;
    const outputCtx = output.getContext('2d', { willReadFrequently: true });
    outputCtx.imageSmoothingEnabled = true;
    outputCtx.imageSmoothingQuality = 'high';
    outputCtx.drawImage(temp, rect.x, rect.y, rect.w, rect.h, 0, 0, outputWidth, outputHeight);

    if (enhance.value) applyEnhance(outputCtx, outputWidth, outputHeight);

    const blob = await new Promise((resolve) => output.toBlob(resolve, 'image/png'));
    if (!blob) throw new Error('生成图片失败');
    status.value = '已完成';
    emit('saved', blob);
  } catch (error) {
    status.value = error.message || '处理失败';
  } finally {
    busy.value = false;
  }
}

function inpaint(data, w, h, marked) {
  const size = w * h;
  const masked = [];
  for (let index = 0; index < size; index += 1) {
    if (marked[index]) masked.push(index);
  }
  if (!masked.length) return;

  const known = new Uint8Array(size);
  for (let index = 0; index < size; index += 1) known[index] = marked[index] ? 0 : 1;

  const queue = new Int32Array(size);
  let head = 0;
  let tail = 0;
  for (const index of masked) {
    const x = index % w;
    const y = (index / w) | 0;
    if (
      (x > 0 && known[index - 1]) ||
      (x < w - 1 && known[index + 1]) ||
      (y > 0 && known[index - w]) ||
      (y < h - 1 && known[index + w])
    ) {
      queue[tail++] = index;
    }
  }

  while (head < tail) {
    const index = queue[head++];
    if (known[index]) continue;
    const x = index % w;
    const y = (index / w) | 0;
    let r = 0;
    let g = 0;
    let b = 0;
    let count = 0;
    for (let dy = -1; dy <= 1; dy += 1) {
      const row = y + dy;
      if (row < 0 || row >= h) continue;
      for (let dx = -1; dx <= 1; dx += 1) {
        const column = x + dx;
        if (column < 0 || column >= w) continue;
        const neighbor = row * w + column;
        if (!known[neighbor]) continue;
        const offset = neighbor * 4;
        r += data[offset];
        g += data[offset + 1];
        b += data[offset + 2];
        count += 1;
      }
    }
    if (!count) continue;
    const offset = index * 4;
    data[offset] = r / count;
    data[offset + 1] = g / count;
    data[offset + 2] = b / count;
    data[offset + 3] = 255;
    known[index] = 1;
    if (x > 0 && !known[index - 1]) queue[tail++] = index - 1;
    if (x < w - 1 && !known[index + 1]) queue[tail++] = index + 1;
    if (y > 0 && !known[index - w]) queue[tail++] = index - w;
    if (y < h - 1 && !known[index + w]) queue[tail++] = index + w;
  }

  const next = new Uint8ClampedArray(data.length);
  const iterations = 20;
  for (let iteration = 0; iteration < iterations; iteration += 1) {
    next.set(data);
    for (const index of masked) {
      const x = index % w;
      const y = (index / w) | 0;
      let r = 0;
      let g = 0;
      let b = 0;
      let count = 0;
      for (let dy = -1; dy <= 1; dy += 1) {
        const row = y + dy;
        if (row < 0 || row >= h) continue;
        for (let dx = -1; dx <= 1; dx += 1) {
          if (dx === 0 && dy === 0) continue;
          const column = x + dx;
          if (column < 0 || column >= w) continue;
          const offset = (row * w + column) * 4;
          r += data[offset];
          g += data[offset + 1];
          b += data[offset + 2];
          count += 1;
        }
      }
      if (!count) continue;
      const offset = index * 4;
      next[offset] = r / count;
      next[offset + 1] = g / count;
      next[offset + 2] = b / count;
    }
    data.set(next);
  }
}

function applyEnhance(ctx, w, h) {
  const imageData = ctx.getImageData(0, 0, w, h);
  const data = imageData.data;
  for (let index = 0; index < data.length; index += 4) {
    let r = data[index];
    let g = data[index + 1];
    let b = data[index + 2];
    if (r > 236 && g > 236 && b > 236) {
      data[index] = 255;
      data[index + 1] = 255;
      data[index + 2] = 255;
      continue;
    }
    r = (r - 128) * 1.045 + 128 + 3;
    g = (g - 128) * 1.045 + 128 + 3;
    b = (b - 128) * 1.045 + 128 + 3;
    data[index] = Math.max(0, Math.min(255, r));
    data[index + 1] = Math.max(0, Math.min(255, g));
    data[index + 2] = Math.max(0, Math.min(255, b));
  }
  ctx.putImageData(imageData, 0, 0);
}

watch([() => crop.top, () => crop.bottom, () => crop.left, () => crop.right], () => {
  const clamp = (value) => Math.max(0, Math.min(45, Number(value) || 0));
  if (crop.left + crop.right > 88) crop.right = 88 - crop.left;
  if (crop.top + crop.bottom > 88) crop.bottom = 88 - crop.top;
  crop.left = clamp(crop.left);
  crop.top = clamp(crop.top);
  draw();
});

watch(showMask, draw);
</script>

<template>
  <div class="modal-backdrop editor-backdrop" @click.self="$emit('close')">
    <div class="editor">
      <header class="modal-head">
        <div>
          <h2>图片净化</h2>
          <p>涂抹水印文字 → 修补 → 裁剪导出，发布时自动使用处理后的图</p>
        </div>
        <UiButton class="icon-btn" @click="$emit('close')">×</UiButton>
      </header>

      <div class="editor-body">
        <div class="canvas-pane">
          <div v-if="loading" class="canvas-loading">加载图片…</div>
          <canvas
            ref="canvasRef"
            @pointerdown="pointerDown"
            @pointermove="pointerMove"
            @pointerup="pointerUp"
            @pointercancel="pointerUp"
          />
          <div class="canvas-tools">
            <UiButton :class="['tool-btn', { active: tool === 'brush' }]" @click="tool = 'brush'">画笔</UiButton>
            <UiButton :class="['tool-btn', { active: tool === 'erase' }]" @click="tool = 'erase'">橡皮</UiButton>
            <UiButton class="tool-btn" :disabled="!history.length" @click="undo">撤销</UiButton>
            <UiButton class="tool-btn" @click="resetAll">重置</UiButton>
            <label class="brush-size">
              笔刷
              <input v-model.number="brushSize" type="range" min="8" max="140" />
            </label>
            <label class="brush-size">
              <input v-model="showMask" type="checkbox" />
              显示标记
            </label>
            <span class="mask-count">{{ maskCount }} px</span>
          </div>
        </div>

        <aside class="editor-side">
          <section>
            <h4>自动识别水印</h4>
            <label class="field">
              <span>检测区域：下方 {{ band }}%</span>
              <input v-model.number="band" type="range" min="10" max="60" />
            </label>
            <label class="field">
              <span>识别强度 {{ threshold }}</span>
              <input v-model.number="threshold" type="range" min="6" max="40" />
            </label>
            <UiButton class="btn small" :disabled="loading || busy" @click="detect">识别并标记</UiButton>
            <p class="hint">识别半透明浅色文字，跑完可再用画笔补涂或橡皮擦掉误标。</p>
          </section>

          <section>
            <h4>裁剪</h4>
            <label class="field">
              <span>上 {{ crop.top }}%</span>
              <input v-model.number="crop.top" type="range" min="0" max="40" />
            </label>
            <label class="field">
              <span>下 {{ crop.bottom }}%</span>
              <input v-model.number="crop.bottom" type="range" min="0" max="40" />
            </label>
            <label class="field">
              <span>左 {{ crop.left }}%</span>
              <input v-model.number="crop.left" type="range" min="0" max="40" />
            </label>
            <label class="field">
              <span>右 {{ crop.right }}%</span>
              <input v-model.number="crop.right" type="range" min="0" max="40" />
            </label>
          </section>

          <section>
            <h4>输出</h4>
            <label class="switch small">
              <input v-model="enhance" type="checkbox" />
              <span class="switch-track"></span>
              <span class="switch-text">白底增强 / 轻微提亮</span>
            </label>
            <p class="hint">输出最长边 1200px 的 PNG，自动替换原图用于发布。</p>
          </section>

          <p class="status-line">{{ status }}</p>
          <UiButton class="btn accent block" :disabled="busy || loading" @click="save">
            {{ busy ? '处理中…' : '保存处理结果' }}
          </UiButton>
          <p class="hint tiny-note">
            仅处理自己拥有使用权的图片。使用第三方平台图片前请确认授权。
          </p>
        </aside>
      </div>
    </div>
  </div>
</template>
