<script setup>
import { computed, reactive, ref, watch } from 'vue';
import { api, assetUrl } from '../api';
import BookOperations from './BookOperations.vue';
import { date, orderStates, yuan } from '../xianyu';

const props = defineProps({
  book: { type: Object, required: true },
  settings: { type: Object, default: () => ({}) },
  publishing: Boolean,
});

const emit = defineEmits([
  'close',
  'open-order',
  'open-fulfillment',
  'save',
  'find',
  'publish',
  'api-publish',
  'upload',
  'import-urls',
  'primary',
  'delete-image',
  'edit',
  'open-taobao',
  'refresh',
]);

const conditions = ['全新', '十成新', '九五新', '九成新', '八成新', '七成新', '六成新', '五成新'];

const form = reactive({
  title: '',
  author: '',
  publisher: '',
  isbn: '',
  edition: '',
  condition: '九成新',
  costPrice: null,
  listPrice: null,
  sellPrice: null,
  stock: 1,
  note: '',
});

const listing = reactive({ title: '', description: '', price: null });
const snapshot = ref('');
const listingTouched = ref(false);
const urlPanel = ref(false);
const urlText = ref('');
const fileInput = ref(null);

const linkedOrders = ref([]),
  ordersError = ref(''),
  ordersLoading = ref(false);
let ordersVersion = 0;
async function loadLinkedOrders(id = props.book.id) {
  const version = ++ordersVersion;
  linkedOrders.value = [];
  ordersError.value = '';
  ordersLoading.value = true;
  try {
    const orders = await api.xianyuBookOrders(id);
    if (version === ordersVersion) linkedOrders.value = orders;
  } catch (e) {
    if (version === ordersVersion) ordersError.value = e.message;
  } finally {
    if (version === ordersVersion) ordersLoading.value = false;
  }
}
watch(() => props.book.id, loadLinkedOrders, { immediate: true });

const images = computed(() => props.book.images || []);
const capture = computed(() => props.book.capture || null);
const latestListing = computed(() => (props.book.listings || [])[0] || null);
const dirty = computed(() => JSON.stringify(form) !== snapshot.value);

const statusText = {
  draft: '待整理',
  ready: '待发布',
  listed: '已上架',
  sold: '已售出',
  filling: '发布中',
  published: '已发布',
  failed: '发布失败',
};

function plainTitle(value) {
  return String(value || '')
    .replace(/^《/, '')
    .replace(/》$/, '')
    .trim();
}

function toNumber(value) {
  if (value === null || value === undefined || value === '') return null;
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function ceilYuan(value) {
  const nearest = Math.round(value);
  return Math.abs(value - nearest) < 1e-9 ? nearest : Math.ceil(value);
}

function sync() {
  const book = props.book || {};
  Object.assign(form, {
    title: book.title || '',
    author: book.author || '',
    publisher: book.publisher || '',
    isbn: book.isbn || '',
    edition: book.edition || '',
    condition: book.condition || '九成新',
    costPrice: book.costPrice ?? null,
    listPrice: book.listPrice ?? null,
    sellPrice: book.sellPrice ?? null,
    stock: book.stock ?? 1,
    note: book.note || '',
  });
  snapshot.value = JSON.stringify(form);
  if (!listingTouched.value) generate();
}

watch(() => props.book, sync, { immediate: true, deep: false });

function generate() {
  const book = props.book || {};
  const title = plainTitle(form.title || book.title) || '未命名图书';
  const author = form.author || book.author;
  const condition = form.condition || book.condition;
  const parts = [`《${title}》`];
  if (author) parts.push(author);
  if (condition) parts.push(condition);
  if (props.settings.titleSuffix) parts.push(props.settings.titleSuffix);
  listing.title = parts.join(' ').slice(0, 30);

  const lines = [`【书名】${title}`];
  if (author) lines.push(`【作者】${author}`);
  if (form.publisher) lines.push(`【出版社】${form.publisher}`);
  if (form.edition) lines.push(`【版次】${form.edition}`);
  if (form.isbn) lines.push(`【ISBN】${form.isbn}`);
  if (condition) lines.push(`【品相】${condition}`);
  if (form.listPrice) lines.push(`【定价】${Number(form.listPrice).toFixed(2)} 元`);
  const cost = toNumber(form.costPrice);
  const parsedRate = Number(props.settings.profitRate ?? 15);
  const parsedFloor = Number(props.settings.minProfit ?? 5);
  const profitRate = Number.isFinite(parsedRate) ? parsedRate : 15;
  const minProfit = Number.isFinite(parsedFloor) ? Math.max(0, parsedFloor) : 5;
  const suggested =
    cost !== null && cost > 0
      ? ceilYuan(Math.max(cost * (1 + profitRate / 100), cost + minProfit))
      : null;
  const price = toNumber(form.sellPrice) ?? suggested;
  if (price) lines.push(`【售价】${price.toFixed(2)} 元`);
  if (form.note) lines.push(`【备注】${form.note}`);
  if (props.settings.descFooter) lines.push('', props.settings.descFooter);

  listing.description = lines.join('\n');
  listing.price = price;
  listingTouched.value = false;
}

function patch() {
  return {
    ...form,
    costPrice: toNumber(form.costPrice),
    listPrice: toNumber(form.listPrice),
    sellPrice: toNumber(form.sellPrice),
    stock: Math.max(0, Number(form.stock) || 0),
  };
}

function save() {
  emit('save', patch());
}

function publish(kind = 'publish') {
  if (!listing.title.trim()) return;
  emit(kind, {
    patch: patch(),
    title: listing.title.trim(),
    description: listing.description.trim(),
    price: toNumber(listing.price),
  });
}

function markTouched() {
  listingTouched.value = true;
}

function submitUrls() {
  const urls = urlText.value
    .split(/[\s,，;；]+/)
    .map((value) => value.trim())
    .filter(Boolean);
  if (!urls.length) return;
  emit('import-urls', { urls, source: 'kongfz' });
  urlText.value = '';
  urlPanel.value = false;
}

function onFiles(event) {
  const files = Array.from(event.target.files || []);
  if (files.length) emit('upload', files);
  event.target.value = '';
}

function money(value) {
  if (value === null || value === undefined || value === '') return '—';
  return `¥${Number(value).toFixed(2)}`;
}
</script>

<template>
  <aside class="drawer">
    <header class="drawer-head">
      <div class="drawer-title">
        <div class="drawer-eyebrow">
          书目 #{{ book.id }}
          <span :class="['status', book.status]">{{ statusText[book.status] || book.status }}</span>
        </div>
        <h2>{{ form.title || '未命名图书' }}</h2>
        <p class="drawer-sub">
          {{
            [form.author, form.publisher, form.isbn].filter(Boolean).join(' · ') ||
            '补全信息后即可找图发布'
          }}
        </p>
      </div>
      <div class="drawer-head-actions">
        <UiButton class="icon-btn" title="刷新" @click="$emit('refresh')">↻</UiButton>
        <UiButton class="icon-btn" title="关闭" @click="$emit('close')">×</UiButton>
      </div>
    </header>

    <div class="drawer-body">
      <section class="block">
        <h3>基本信息</h3>
        <div class="form-grid two">
          <label class="field span-2">
            <span>书名</span>
            <input v-model="form.title" class="input" placeholder="红楼梦" />
          </label>
          <label class="field">
            <span>作者</span>
            <input v-model="form.author" class="input" placeholder="曹雪芹" />
          </label>
          <label class="field">
            <span>出版社</span>
            <input v-model="form.publisher" class="input" placeholder="人民文学出版社" />
          </label>
          <label class="field">
            <span>ISBN</span>
            <input v-model="form.isbn" class="input mono" placeholder="9787020002207" />
          </label>
          <label class="field">
            <span>版次</span>
            <input v-model="form.edition" class="input" placeholder="第 3 版" />
          </label>
          <label class="field">
            <span>品相</span>
            <SearchSelect v-model="form.condition" class="input">
              <option v-for="item in conditions" :key="item" :value="item">{{ item }}</option>
            </SearchSelect>
          </label>
          <label class="field">
            <span>历史本地库存</span>
            <input v-model="form.stock" type="number" min="0" class="input" />
          </label>
          <label class="field">
            <span>成本价</span>
            <input v-model="form.costPrice" type="number" step="0.01" class="input" />
          </label>
          <label class="field">
            <span>售价</span>
            <input v-model="form.sellPrice" type="number" step="0.01" class="input" />
          </label>
          <label class="field">
            <span>原定价</span>
            <input v-model="form.listPrice" type="number" step="0.01" class="input" />
          </label>
          <label class="field span-2">
            <span>备注</span>
            <textarea
              v-model="form.note"
              class="input textarea"
              rows="2"
              placeholder="品相细节、缺页、划线等"
            />
          </label>
        </div>
      </section>

      <BookOperations
        :book-id="book.id"
        @updated="emit('refresh')"
        @open-fulfillment="(id) => emit('open-fulfillment', id)"
      />
      <section v-if="capture" class="block capture-block">
        <div class="block-head">
          <h3>淘宝采集</h3>
          <UiButton class="link-btn" @click="$emit('open-taobao')">打开原页 ↗</UiButton>
        </div>
        <div class="capture-grid">
          <div>
            <span>抓取价</span><strong>{{ money(capture.price) }}</strong>
          </div>
          <div>
            <span>店铺</span><strong>{{ capture.shop || '—' }}</strong>
          </div>
          <div>
            <span>销量</span><strong>{{ capture.sales || '—' }}</strong>
          </div>
          <div>
            <span>抓取时间</span
            ><strong>{{ new Date(capture.updatedAt).toLocaleString('zh-CN') }}</strong>
          </div>
        </div>
      </section>

      <section class="block">
        <div class="block-head">
          <h3>
            图片 <small>{{ images.length }} 张</small>
          </h3>
          <div class="block-actions">
            <UiButton class="btn tiny ghost" @click="$emit('find')">孔夫子找图</UiButton>
            <UiButton class="btn tiny ghost" @click="fileInput?.click()">上传</UiButton>
            <UiButton class="btn tiny ghost" @click="urlPanel = !urlPanel">链接导入</UiButton>
          </div>
        </div>

        <input ref="fileInput" type="file" accept="image/*" multiple hidden @change="onFiles" />

        <div v-if="urlPanel" class="url-import">
          <textarea
            v-model="urlText"
            class="input textarea"
            rows="3"
            placeholder="粘贴图片直链，一行一个（或逗号分隔）"
          />
          <div class="url-import-actions">
            <span>下载后可在图片上点「净化」处理水印和瑕疵</span>
            <UiButton class="btn tiny" @click="submitUrls">导入</UiButton>
          </div>
        </div>

        <div v-if="!images.length" class="image-empty">
          <p>还没有图片</p>
          <span>去孔夫子搜同款，选几张实拍图导入</span>
        </div>

        <div v-else class="thumb-grid">
          <div
            v-for="image in images"
            :key="image.id"
            :class="['thumb', { primary: image.isPrimary }]"
          >
            <img
              :src="assetUrl(image.processed ? image.processedUrl : image.url)"
              alt=""
              loading="lazy"
            />
            <span v-if="image.isPrimary" class="thumb-badge">主图</span>
            <span v-if="image.processed" class="thumb-badge processed">已净化</span>
            <div class="thumb-actions">
              <UiButton @click="$emit('edit', image)">净化</UiButton>
              <UiButton v-if="!image.isPrimary" @click="$emit('primary', image)">主图</UiButton>
              <UiButton class="danger" @click="$emit('delete-image', image)">删除</UiButton>
            </div>
          </div>
          <UiButton class="thumb add" @click="fileInput?.click()">
            <span>＋</span>
            <small>上传图片</small>
          </UiButton>
        </div>
      </section>

      <section class="block">
        <div class="block-head">
          <h3>关联闲鱼订单</h3>
          <UiButton class="link-btn" @click="loadLinkedOrders()">刷新记录</UiButton>
        </div>
        <p v-if="ordersLoading" class="hint">正在载入关联订单…</p>
        <p v-else-if="ordersError" class="xy-error" role="alert">{{ ordersError }}</p>
        <p v-else-if="!linkedOrders.length" class="hint">
          暂无已同步的关联订单。先在闲鱼工作台查询订单，匹配成功或手动关联后会显示在这里。
        </p>
        <div v-for="order in linkedOrders" :key="order.order_no" class="block">
          <UiButton class="link-btn mono" @click="$emit('open-order', order.order_no)">
            {{ order.order_no }} · 查看实时详情
          </UiButton>
          <p>
            {{ orderStates[order.order_status] || order.order_status }} · 数量
            {{ order.goods?.quantity ?? '—' }} · 实付 {{ yuan(order.pay_amount) }}
          </p>
          <small class="hint"
            >下单 {{ date(order.order_time) }} · 同步于
            {{ new Date(order.syncedAt).toLocaleString('zh-CN') }}</small
          >
        </div>
      </section>
      <section class="block">
        <div class="block-head">
          <h3>发布内容</h3>
          <UiButton class="link-btn" @click="generate()">重新生成文案</UiButton>
        </div>

        <div v-if="latestListing" class="listing-state">
          <span :class="['status', latestListing.status]">
            {{ statusText[latestListing.status] || latestListing.status }}
          </span>
          <span class="listing-time">{{
            new Date(latestListing.updatedAt).toLocaleString('zh-CN')
          }}</span>
          <span v-if="latestListing.error" class="listing-error">{{ latestListing.error }}</span>
        </div>

        <label class="field">
          <span
            >闲鱼标题 <em>{{ listing.title.length }}/30</em></span
          >
          <input v-model="listing.title" maxlength="30" class="input" @input="markTouched" />
        </label>

        <label class="field">
          <span>售价</span>
          <input
            v-model="listing.price"
            type="number"
            step="0.01"
            class="input"
            @input="markTouched"
          />
        </label>

        <label class="field">
          <span>描述</span>
          <textarea
            v-model="listing.description"
            rows="10"
            class="input textarea"
            @input="markTouched"
          />
        </label>

        <p class="hint">
          发布时会打开闲鱼商品发布页，脚本自动上传图片、填写价格/描述，并按设置提交。新版网页没有独立标题，标题会写到描述首行；网页不支持的商品分类会自动尝试改为「其他闲置」。
        </p>
      </section>
    </div>

    <footer class="drawer-foot">
      <UiButton class="btn ghost" @click="$emit('find')">找图</UiButton>
      <div class="spacer"></div>
      <UiButton class="btn" :disabled="!dirty" @click="save">保存修改</UiButton>
      <UiButton class="btn accent" :disabled="publishing || !images.length" @click="publish()">
        {{ publishing ? '处理中…' : '网页发布' }}
      </UiButton>
      <UiButton class="btn ghost" :disabled="publishing" @click="publish('api-publish')">
        接口发布
      </UiButton>
    </footer>
  </aside>
</template>
