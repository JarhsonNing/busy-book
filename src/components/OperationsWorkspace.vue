<script setup>
import { computed, nextTick, reactive, ref, watch } from 'vue';
import { api, openBrowser } from '../api';
import { emptySource, fulfillmentStates, kindName, showTime, yearDifference } from '../commerce';
const props = defineProps({ view: String });
const emit = defineEmits(['open-book', 'updated', 'navigate']);
const busy = ref(false),
  error = ref(''),
  message = ref(''),
  overview = ref(null),
  books = ref([]),
  requirements = ref({ records: [], schools: [] }),
  sources = ref([]),
  orders = ref([]);
const school = ref(''),
  year = ref(''),
  major = ref(''),
  stage = ref(''),
  onlyPending = ref(false),
  priorYear = ref('');
const importData = ref(null),
  preview = ref(null),
  confirming = ref(null),
  bookSearch = ref('');
const confirmation = reactive({ bookId: null, editionChecked: false, book: {} });
const source = reactive({ id: null, bookId: null, primary: false, data: emptySource() });
const detail = ref(null),
  remote = ref(null),
  orderFilter = ref(''),
  orderSearch = ref(''),
  purchaseSources = ref([]);
const purchase = reactive({
  sourceId: null,
  state: 'draft',
  purchaseNo: '',
  actualCost: null,
  trackingNo: '',
  expressCode: '',
  expressName: '',
  note: '',
  confirmed: false,
});
const finance = reactive({ cost: null, fees: null, refund: null }),
  note = ref(''),
  closeCase = ref(false);
const draftBooks = ref([]),
  draftShop = ref(''),
  draftShops = ref([]);
async function loadDraftShops() {
  await act(async () => {
    draftShops.value = (await api.xianyuCall('shops')).list || [];
  });
}
async function prepareDrafts() {
  await act(async () => {
    let count = 0;
    const failures = [];
    for (const id of draftBooks.value) {
      const b = books.value.find((b) => b.id === id);
      try {
        await api.commerce('/channels', {
          bookId: id,
          shop: draftShop.value,
          createOnly: true,
          data: {
            mode: 'mixed',
            title: b.title,
            content:
              '电子版：对应版次的电子资料。实体版：纸质图书，货源代发。请按所需形态选择规格。',
            physicalPrice: null,
            digitalPrice: null,
            physicalStock: 1,
            digitalStock: 100,
            resourceId: null,
          },
        });
        count++;
      } catch (e) {
        failures.push(b.title + '：' + e.message);
      }
    }
    message.value = `已准备 ${count} 条混合商品草稿。${failures.join('；')}`;
    draftBooks.value = [];
  });
}
const confirmationPanel = ref(null),
  loadingOrder = ref(false);
let orderVersion = 0;
let loadVersion = 0;
const matches = computed(() =>
  books.value.filter(
    (b) =>
      !bookSearch.value ||
      [b.title, b.author, b.isbn, b.edition]
        .join(' ')
        .toLowerCase()
        .includes(bookSearch.value.toLowerCase()),
  ),
);
const schoolNames = computed(() => [
  ...new Set(requirements.value.records.map((r) => r.data.school)),
]);
const years = computed(() =>
  [
    ...new Set(
      requirements.value.records
        .filter((r) => !school.value || r.data.school === school.value)
        .map((r) => r.data.year),
    ),
  ].sort((a, b) => b - a),
);
const filteredRequirements = computed(() =>
  requirements.value.records.filter(
    (r) =>
      (!school.value || r.data.school === school.value) &&
      (!year.value || r.data.year === Number(year.value)) &&
      (!major.value ||
        [r.data.college, r.data.major, r.data.subject].join(' ').includes(major.value)) &&
      (!stage.value || r.data.stage === stage.value) &&
      (!onlyPending.value || r.state !== 'confirmed'),
  ),
);
const stages = computed(() => [...new Set(requirements.value.records.map((r) => r.data.stage))]);
const difference = computed(() =>
  school.value && year.value && priorYear.value
    ? yearDifference(requirements.value.records, school.value, year.value, priorYear.value)
    : null,
);
const filteredOrders = computed(() =>
  orders.value.filter(
    (f) =>
      (!orderFilter.value || f.state === orderFilter.value) &&
      (!orderSearch.value ||
        [f.orderNo, f.snapshot.goods?.title].join(' ').includes(orderSearch.value)),
  ),
);
const money = (n) => (n == null ? '待核算' : `¥${(n / 100).toFixed(2)}`);
async function refresh() {
  const run = ++loadVersion;
  error.value = '';
  try {
    const [o, b] = await Promise.all([api.commerce('/overview'), api.commerce('/books')]);
    if (run !== loadVersion) return;
    overview.value = o;
    books.value = b;
    if (props.view === 'schools') {
      const r = await api.commerce('/requirements');
      if (run === loadVersion) requirements.value = r;
    }
    if (props.view === 'sources') {
      const r = await api.commerce('/sources');
      if (run === loadVersion) sources.value = r;
    }
    if (props.view === 'fulfillment') {
      const r = await api.commerce('/fulfillments');
      if (run === loadVersion) orders.value = r;
    }
  } catch (e) {
    if (run === loadVersion) error.value = e.message;
  }
}
watch(
  () => props.view,
  () => {
    orderVersion++;
    loadingOrder.value = false;
    detail.value = null;
    remote.value = null;
    refresh();
  },
  { immediate: true },
);
async function act(work) {
  if (busy.value) return;
  busy.value = true;
  error.value = '';
  message.value = '';
  try {
    await work();
    await refresh();
    emit('updated');
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
async function fileChanged(e) {
  const file = e.target.files?.[0];
  preview.value = null;
  importData.value = null;
  if (!file) return;
  await act(async () => {
    if (file.size > 30 * 1024 * 1024) throw Error('目录文件过大，请分批导入');
    const data = JSON.parse(await file.text());
    preview.value = await api.commerce('/catalog/preview', data);
    importData.value = data;
  });
  e.target.value = '';
}
async function importCatalog() {
  await act(async () => {
    const result = await api.commerce('/catalog/import', importData.value);
    message.value = `导入完成：新增 ${result.added}，变更待确认 ${result.changed}，未变 ${result.unchanged}`;
    preview.value = null;
    importData.value = null;
  });
}
async function confirmRequirement(r) {
  confirming.value = r;
  bookSearch.value = r.data.candidate.title || '';
  const c = r.data.candidate;
  Object.assign(confirmation, {
    bookId: r.bookId || null,
    editionChecked: false,
    book: {
      title: c.title || '',
      author: Array.isArray(c.authors) ? c.authors.join('、') : c.authors || '',
      publisher: c.publisher || '',
      isbn: c.isbn || '',
      edition: r.data.ambiguous ? '' : c.edition || '',
      volume: r.data.ambiguous ? '' : c.volume || '',
      publicationYear: String(c.year || ''),
    },
  });
  await nextTick();
  confirmationPanel.value?.scrollIntoView({ behavior: 'smooth', block: 'center' });
}
async function saveConfirmation() {
  await act(async () => {
    await api.commerce('/requirements/' + confirming.value.id + '/confirm', confirmation);
    confirming.value = null;
    message.value = '已关联标准书目';
  });
}
function editSource(s) {
  Object.assign(source, {
    id: s?.id || null,
    bookId: s?.bookId || null,
    primary: s?.primary || false,
    data: { ...emptySource(), ...s?.data },
  });
  bookSearch.value = '';
}
async function saveSource() {
  await act(async () => {
    await api.commerce('/sources', {
      ...source,
      data: {
        ...source.data,
        price: source.data.price === '' ? null : source.data.price,
        freight: source.data.freight === '' ? null : source.data.freight,
      },
    });
    editSource();
    message.value = '货源已保存';
  });
}
async function syncOrders() {
  await act(async () => {
    const r = await api.commerce('/sync-orders', {});
    message.value = `同步 ${r.count} 条订单。${r.deliveryErrors?.length ? '部分电子交付查询失败，请打开订单核查。' : ''}`;
  });
}
async function openOrder(id) {
  const run = ++orderVersion;
  loadingOrder.value = true;
  detail.value = null;
  remote.value = null;
  error.value = '';
  try {
    const live = await api
      .commerce('/fulfillments/' + id + '/refresh', {})
      .catch((e) => ({ error: e.message, stock: null, skuStock: null }));
    const f = await api.commerce('/fulfillments/' + id);
    const allSources = f.bookId ? (await api.commerce('/books/' + f.bookId)).sources : [];
    if (run !== orderVersion) return;
    purchaseSources.value = allSources;
    const current = f.purchases.find((p) => !['canceled', 'replaced'].includes(p.state));
    Object.assign(
      purchase,
      {
        sourceId: f.data.source?.id || null,
        state: 'draft',
        purchaseNo: '',
        actualCost: null,
        supplierRefund: null,
        trackingNo: '',
        expressCode: '',
        expressName: '',
        note: '',
        confirmed: false,
      },
      current?.data || {},
      current ? { state: current.state, sourceId: current.data.source?.id } : {},
    );
    purchase.confirmed = false;
    Object.assign(finance, { cost: null, fees: null, refund: null }, f.data.finance || {});
    note.value = '';
    closeCase.value = false;
    detail.value = f;
    remote.value = live;
  } catch (e) {
    if (run === orderVersion) error.value = e.message;
  } finally {
    if (run === orderVersion) loadingOrder.value = false;
  }
}
async function savePurchase() {
  await act(async () => {
    await api.commerce('/fulfillments/' + detail.value.id + '/purchase', purchase);
    detail.value = await api.commerce('/fulfillments/' + detail.value.id);
    message.value = '采购进度已保存';
  });
}
async function saveFinance() {
  await act(() =>
    api.commerce(
      '/fulfillments/' + detail.value.id + '/finance',
      Object.fromEntries(Object.entries(finance).map(([k, v]) => [k, v === '' ? null : v])),
    ),
  );
}
async function saveNote() {
  await act(async () => {
    await api.commerce('/fulfillments/' + detail.value.id + '/note', {
      note: note.value,
      close: closeCase.value,
    });
    detail.value = await api.commerce('/fulfillments/' + detail.value.id);
    note.value = '';
    closeCase.value = false;
  });
}
async function ship() {
  if (!window.confirm('已核对淘宝物流单号与本订单收件信息，确认回传闲管家发货？')) return;
  await act(async () => {
    const r = await api.commerce('/fulfillments/' + detail.value.id + '/ship', {});
    message.value = r.message || '远端已发货';
    await openOrder(detail.value.id);
  });
}
function openTask(t) {
  if (t.type === 'book') emit('open-book', t.bookId);
  else {
    emit('navigate', 'fulfillment', t.fulfillmentId);
  }
}
defineExpose({ refresh, openOrder });
</script>

<template>
  <section class="panel operations-panel">
    <header class="operations-head">
      <div>
        <h2>
          {{
            {
              home: '今日待办',
              schools: '学校书单',
              sources: '实体货源',
              fulfillment: '订单与履约',
              reports: '经营统计',
            }[view]
          }}
        </h2>
        <p class="hint">学校用书 → 标准书目 → 资源与货源 → 销售商品 → 订单履约</p>
      </div>
      <div class="head-actions">
        <UiButton
          v-if="['home', 'fulfillment'].includes(view)"
          class="btn"
          :disabled="busy"
          @click="syncOrders"
        >
          {{ busy ? '处理中…' : '同步渠道订单' }}</UiButton
        ><UiButton class="btn ghost" :disabled="busy" @click="refresh">刷新</UiButton>
      </div>
    </header>
    <p v-if="error" class="xy-error" role="alert">{{ error }}</p>
    <p v-if="message" role="status" class="business-notice">{{ message }}</p>
    <template v-if="view === 'home' && overview">
      <div class="business-metrics">
        <UiButton class="business-card" @click="emit('navigate', 'schools')">
          <small>待确认学校书目</small><strong>{{ overview.pendingRequirements }}</strong>
        </UiButton>
        <div class="business-card">
          <small>经营待办</small><strong>{{ overview.tasks.length }}</strong>
        </div>
        <div class="business-card">
          <small>标准书目</small><strong>{{ books.length }}</strong>
        </div>
      </div>
      <details class="business-notice">
        <summary>混合规格自动交付：尚待平台验证，混合商品保持草稿</summary>
        <p>{{ overview.capability.message }}</p>
        <ul>
          <li v-for="c in overview.capability.checks" :key="c.name">{{ c.name }} · 待验证</li>
        </ul>
      </details>
      <details class="business-card">
        <summary>批量准备销售草稿</summary>
        <form class="business-form" @submit.prevent="prepareDrafts">
          <p class="hint">
            同店铺同书已有商品或草稿会跳过。混合商品保留草稿，后续逐书补价格、资源与发布资料。
          </p>
          <UiButton type="button" class="btn ghost" :disabled="busy" @click="loadDraftShops">
            读取闲鱼店铺</UiButton
          ><SearchSelect v-model="draftShop" class="input" required aria-label="批量草稿店铺">
            <option value="">选择店铺</option>
            <option v-for="s in draftShops" :key="s.user_name" :value="s.user_name">
              {{ s.shop_name || s.user_name }}
            </option>
          </SearchSelect>
          <div class="business-scroll">
            <label v-for="b in books" :key="b.id" class="xy-check"
              ><input v-model="draftBooks" type="checkbox" :value="b.id" />{{ b.title }} ·
              {{ b.edition }}</label
            >
          </div>
          <UiButton class="btn" :disabled="busy || !draftBooks.length">
            准备 {{ draftBooks.length }} 本书的草稿
          </UiButton>
        </form>
      </details>
      <div v-if="!overview.tasks.length" class="empty">暂无经营待办，可从学校书单导入开始。</div>
      <UiButton v-for="(t, i) in overview.tasks" :key="i" class="business-task" @click="openTask(t)">
        <div>
          <strong>{{ t.title || t.orderNo }}</strong>
          <p>{{ fulfillmentStates[t.reason] || t.reason }}</p>
        </div>
        <span>{{ showTime(t.updatedAt) }} →</span>
      </UiButton>
    </template>
    <template v-if="view === 'schools'">
      <div class="business-notice">
        选择采集结果
        <code>data/kaoyan/catalog/catalog.json</code>
        进行预览。重复导入不会复制已确认的书目；新年度保留独立适用记录。
      </div>
      <label class="field"
        ><span>导入采集目录 JSON</span
        ><input type="file" accept=".json,application/json" :disabled="busy" @change="fileChanged"
      /></label>
      <div v-if="preview" class="business-card">
        <h3>导入预览 · {{ preview.count }} 条</h3>
        <p>
          新增 {{ preview.records.filter((r) => r.importAction === 'new').length }} · 变更
          {{ preview.records.filter((r) => r.importAction === 'changed').length }} · 未变
          {{ preview.records.filter((r) => r.importAction === 'unchanged').length }}
        </p>
        <div class="business-scroll">
          <p v-for="r in preview.records" :key="r.id">
            {{ r.school }} · {{ r.year }} · {{ r.major }} · {{ r.raw }}
            <strong v-if="r.ambiguous">（聚合了多个版次／册次，须重新确认）</strong>
          </p>
        </div>
        <UiButton class="btn" :disabled="busy" @click="importCatalog">确认导入待确认区</UiButton>
      </div>
      <div class="business-filters">
        <label
          >学校<SearchSelect v-model="school" class="input">
            <option value="">全部学校</option>
            <option v-for="n in schoolNames" :key="n">{{ n }}</option>
          </SearchSelect></label
        ><label
          >招生年份<SearchSelect v-model="year" class="input">
            <option value="">全部年份</option>
            <option v-for="n in years" :key="n" :value="n">{{ n }}</option>
          </SearchSelect></label
        ><label
          >学院／专业／科目<input v-model="major" class="input" placeholder="输入关键词" /></label
        ><label
          >考试阶段<SearchSelect v-model="stage" class="input">
            <option value="">全部</option>
            <option v-for="n in stages" :key="n">{{ n }}</option>
          </SearchSelect></label
        ><label class="xy-check"><input v-model="onlyPending" type="checkbox" />仅待确认</label>
      </div>
      <div class="business-metrics">
        <div
          v-for="s in requirements.schools.filter(
            (s) => (!school || s.school === school) && (!year || s.year === Number(year)),
          )"
          :key="s.school + s.year"
          class="business-card"
        >
          <strong>{{ s.school }} · {{ s.year }}</strong>
          <p>{{ s.confirmed }} / {{ s.total }} 已建档</p>
          <small
            >有货源 {{ s.physical }} · 有电子资源 {{ s.digital }} · 已上架 {{ s.listed }}</small
          >
        </div>
      </div>
      <details v-if="school && year" class="business-card">
        <summary>年度书单增删对比</summary>
        <SearchSelect v-model="priorYear" class="input" aria-label="对比年份">
          <option value="">选择上一年度</option>
          <option v-for="n in years.filter((n) => n !== Number(year))" :key="n" :value="n">
            {{ n }}
          </option></SearchSelect
        ><template v-if="difference"
          ><p>
            相较 {{ priorYear }} 年：新增 {{ difference.added.length }}，移除
            {{ difference.removed.length }}（按原始书目文字与科目比较）
          </p>
          <p v-for="r in difference.added" :key="'a' + r.id">
            ＋ {{ r.data.major }} / {{ r.data.raw }}
          </p>
          <p v-for="r in difference.removed" :key="'r' + r.id">
            － {{ r.data.major }} / {{ r.data.raw }}
          </p></template
        >
      </details>
      <div class="business-scroll">
        <article v-for="r in filteredRequirements" :key="r.id" class="business-card">
          <div class="business-row">
            <strong
              >{{ r.data.school }} · {{ r.data.year }} · {{ r.data.college }} /
              {{ r.data.major }}</strong
            ><span>{{ r.state === 'confirmed' ? '已关联' : '待确认' }}</span>
          </div>
          <p>{{ r.data.stage }} · {{ r.data.subjectCode }} {{ r.data.subject }}</p>
          <p>
            <strong>候选书名：{{ r.data.candidate.title }}</strong>
          </p>
          <p>{{ r.data.raw }}</p>
          <p v-if="r.data.ambiguous" class="xy-error">
            原采集聚合了多个版本或册次，请按原文重新确认。
          </p>
          <p class="hint">采集：{{ r.data.collectedAt || '未知' }} · {{ r.data.sourceFile }}</p>
          <div class="head-actions">
            <a
              v-if="/^https?:\/\//.test(r.data.sourceUrl)"
              :href="r.data.sourceUrl"
              target="_blank"
              rel="noreferrer"
              >来源页面</a
            ><UiButton class="btn tiny" @click="confirmRequirement(r)">核对并关联书目</UiButton
            ><UiButton v-if="r.bookId" class="btn tiny ghost" @click="emit('open-book', r.bookId)">
              打开标准书目
            </UiButton>
          </div>
        </article>
        <p v-if="!filteredRequirements.length" class="empty">没有符合条件的用书记录</p>
      </div>
      <form
        v-if="confirming"
        ref="confirmationPanel"
        class="business-form business-card"
        @submit.prevent="saveConfirmation"
      >
        <h3>确认书目关联</h3>
        <p>{{ confirming.data.raw }}</p>
        <label class="field"
          ><span>搜索已有标准书目（优先复用同版同册）</span
          ><input v-model="bookSearch" class="input" /></label
        ><SearchSelect v-model="confirmation.bookId" class="input" aria-label="关联标准书目">
          <option :value="null">没有对应版本，人工建立新书目</option>
          <option v-for="b in matches" :key="b.id" :value="b.id">
            {{ b.title }} · {{ b.edition }} · {{ b.profile.volume }} · {{ b.isbn || '缺 ISBN' }}
          </option>
        </SearchSelect>
        <div v-if="!confirmation.bookId" class="form-grid two">
          <label
            v-for="(label, key) in {
              title: '书名',
              author: '作者',
              publisher: '出版社',
              isbn: 'ISBN',
              edition: '版次',
              volume: '册次',
              publicationYear: '出版年份',
            }"
            :key="key"
            class="field"
            ><span>{{ label }}</span
            ><input v-model="confirmation.book[key]" :required="key === 'title'" class="input"
          /></label>
        </div>
        <label class="xy-check"
          ><input
            v-model="confirmation.editionChecked"
            type="checkbox"
            required
          />已核对原文，确认版次与册次对应；不合并无法确认的同名书</label
        >
        <div class="head-actions">
          <UiButton class="btn" :disabled="busy">确认关联</UiButton
          ><UiButton type="button" class="btn ghost" @click="confirming = null">取消</UiButton>
        </div>
      </form>
    </template>
    <template v-if="view === 'sources'">
      <p class="hint">淘宝采集进入货源候选。先选择标准书目，再核对版本、采购价、运费与代发能力。</p>
      <div class="business-columns">
        <div class="business-scroll">
          <article v-for="s in sources" :key="s.id" class="business-card">
            <strong>{{ s.data.title || s.data.shop }}</strong>
            <p>
              {{ s.bookId ? books.find((b) => b.id === s.bookId)?.title : '尚未关联图书' }} ·
              {{ s.primary ? '主选' : '备选 / 候选' }}
            </p>
            <p>{{ s.data.sku || '规格待核对' }} · ¥{{ s.data.price ?? '待核价' }}</p>
            <UiButton class="btn tiny" @click="editSource(s)">关联 / 编辑货源</UiButton>
          </article>
          <p v-if="!sources.length" class="empty">还没有货源，可打开淘宝采集或手动录入。</p>
        </div>
        <form class="business-form" @submit.prevent="saveSource">
          <h3>{{ source.id ? '编辑货源' : '手动新增货源' }}</h3>
          <input
            v-model="bookSearch"
            class="input"
            placeholder="搜索关联图书"
            aria-label="搜索关联图书"
          /><label class="field"
            ><span>标准书目</span
            ><SearchSelect v-model="source.bookId" class="input">
              <option :value="null">暂不关联</option>
              <option v-for="b in matches" :key="b.id" :value="b.id">
                {{ b.title }} · {{ b.edition }} · {{ b.isbn }}
              </option>
            </SearchSelect></label
          ><label
            v-for="(label, key) in {
              url: '淘宝链接',
              shop: '店铺',
              sku: '规格 / ISBN / 版次',
              condition: '品相',
            }"
            :key="key"
            class="field"
            ><span>{{ label }}</span
            ><input v-model="source.data[key]" :required="key === 'url'" class="input"
          /></label>
          <div class="form-grid two">
            <label class="field"
              ><span>采购单价（元）</span
              ><input
                v-model.number="source.data.price"
                class="input"
                type="number"
                min="0"
                step="0.01" /></label
            ><label class="field"
              ><span>运费（元）</span
              ><input
                v-model.number="source.data.freight"
                class="input"
                type="number"
                min="0"
                step="0.01"
            /></label>
          </div>
          <label class="xy-check"><input v-model="source.primary" type="checkbox" />主选货源</label
          ><label class="xy-check"
            ><input v-model="source.data.dropship" type="checkbox" />支持代发</label
          ><SearchSelect v-model="source.data.status" class="input" aria-label="货源状态">
            <option value="pending">待核验</option>
            <option value="available">可用</option>
            <option value="unavailable">缺货 / 不可用</option></SearchSelect
          ><UiButton
            type="button"
            class="btn ghost"
            @click="source.data.checkedAt = new Date().toISOString()"
          >
            本次已核验 · {{ showTime(source.data.checkedAt) }}</UiButton
          ><UiButton class="btn" :disabled="busy">保存货源</UiButton>
        </form>
      </div>
    </template>
    <template v-if="view === 'fulfillment'">
      <p v-if="loadingOrder" class="hint" role="status">正在实时核查订单、规格库存和交付结果…</p>
      <div class="business-filters">
        <input
          v-model="orderSearch"
          class="input"
          aria-label="搜索履约订单"
          placeholder="订单号 / 商品标题"
        /><SearchSelect v-model="orderFilter" class="input" aria-label="履约状态">
          <option value="">全部履约状态</option>
          <option v-for="(label, key) in fulfillmentStates" :key="key" :value="key">
            {{ label }}
          </option>
        </SearchSelect>
      </div>
      <div class="business-columns">
        <div class="business-scroll">
          <UiButton
            v-for="f in filteredOrders"
            :key="f.id"
            class="business-task"
            :disabled="busy || loadingOrder"
            @click="openOrder(f.id)"
          >
            <div>
              <strong>{{ f.snapshot.goods?.title || f.orderNo }}</strong>
              <p>{{ kindName(f.kind) }} · {{ fulfillmentStates[f.state] || f.state }}</p>
              <small>{{ f.orderNo }} · {{ money(f.snapshot.pay_amount) }}</small>
            </div>
          </UiButton>
          <p v-if="!filteredOrders.length" class="empty">暂无订单，点击上方同步渠道订单。</p>
        </div>
        <div v-if="detail" class="business-order-detail">
          <h3>{{ detail.orderNo }}</h3>
          <p>{{ kindName(detail.kind) }} · {{ fulfillmentStates[detail.state] }}</p>
          <UiButton v-if="detail.bookId" class="btn tiny" @click="emit('open-book', detail.bookId)">
            打开关联图书</UiButton
          ><UiButton class="btn tiny ghost" @click="openOrder(detail.id)">实时核查订单及库存</UiButton>
          <div class="business-card">
            <strong>实时渠道库存</strong
            ><template v-if="remote"
              ><p v-if="remote.error" class="xy-error">查询失败：{{ remote.error }}</p>
              <p v-else>
                商品总库存 {{ remote.stock ?? '未知' }} · 本单规格库存
                {{ remote.skuStock ?? '未知' }}
              </p>
              <p class="hint">{{ showTime(remote.checkedAt) }}</p>
              <p v-if="remote.skuError || remote.deliveryError" class="xy-error">
                {{ remote.skuError || remote.deliveryError }}
              </p>
              <details v-if="remote.order">
                <summary>本次远端订单详情（核对收件信息）</summary>
                <pre class="business-json">{{ JSON.stringify(remote.order, null, 2) }}</pre>
              </details></template
            >
            <p v-else>正在查询…</p>
          </div>
          <p v-if="detail.kind === 'unknown'" class="business-notice">
            未找到准确规格映射。请从图书的「销售商品」核对店铺、商品及规格
            ID，再同步本订单。不能根据标题确定履约类型。
          </p>
          <form
            v-if="detail.kind === 'physical'"
            class="business-form"
            @submit.prevent="savePurchase"
          >
            <h3>实体采购代发</h3>
            <p>
              购买数量：{{
                detail.snapshot.goods?.quantity ?? '未知，请核查'
              }}。报价仅供参考，下单时再次确认。
            </p>
            <label class="field"
              ><span>本次货源</span
              ><SearchSelect v-model="purchase.sourceId" class="input" required>
                <option :value="null">选择货源</option>
                <option v-for="s in purchaseSources" :key="s.id" :value="s.id">
                  {{ s.primary ? '主选 · ' : '' }}{{ s.data.shop }} · {{ s.data.sku }} · ¥{{
                    s.data.price
                  }}
                  · {{ s.data.status }}
                </option>
              </SearchSelect></label
            ><UiButton
              type="button"
              class="btn ghost"
              :disabled="!purchase.sourceId"
              @click="
                openBrowser(
                  'taobao',
                  purchaseSources.find((s) => s.id === purchase.sourceId)?.data.url,
                )
              "
            >
              打开淘宝，人工下单付款</UiButton
            ><SearchSelect v-model="purchase.state" class="input" aria-label="采购进度">
              <option value="draft">待下单</option>
              <option value="ordered">已采购待物流</option>
              <option value="shipped">货源已发货</option>
              <option value="canceled">采购已取消 / 退货完成</option></SearchSelect
            ><label
              v-for="(label, key) in {
                purchaseNo: '淘宝采购单号',
                trackingNo: '物流单号',
                expressCode: '物流公司编码',
                expressName: '物流公司名称',
                note: '采购备注 / 缺货替换或取消原因',
              }"
              :key="key"
              class="field"
              ><span>{{ label }}</span
              ><input v-model="purchase[key]" class="input" /></label
            ><label class="field"
              ><span>实际采购总额（含运费，元）</span
              ><input
                v-model.number="purchase.actualCost"
                class="input"
                type="number"
                min="0"
                step="0.01" /></label
            ><label v-if="purchase.state === 'canceled'" class="field"
              ><span>供应商实际退回金额（元，未付款填 0）</span
              ><input
                v-model.number="purchase.supplierRefund"
                type="number"
                min="0"
                step="0.01"
                class="input"
                required
            /></label>
            <label class="xy-check"
              ><input
                v-model="purchase.confirmed"
                type="checkbox"
              />已核对版次、数量、收件信息与金额；取消时已核实淘宝处理结果</label
            ><UiButton class="btn" :disabled="busy">保存采购记录</UiButton
            ><UiButton
              v-if="detail.state === 'ready_to_ship'"
              type="button"
              class="btn"
              :disabled="busy"
              @click="ship"
            >
              核对并回传物流
            </UiButton>
            <p v-if="detail.state === 'shipping_unknown'" class="business-notice">
              上次发货结果未知，先刷新核查远端状态，禁止重复提交。
            </p>
          </form>
          <div v-if="detail.kind === 'digital'" class="business-card">
            <h3>电子交付核查</h3>
            <p>
              关联资源版本（实际交付内容待核对）：{{ detail.data.resource?.version || '尚未确认' }}
            </p>
            <p>
              只有查询到实际发卡结果才记录交付完成。托管配置与关机发货验证通过前，不开放混合商品上架。
            </p>
            <p v-for="d in detail.deliveries" :key="d.id">
              {{ showTime(d.createdAt) }} · 已核查发卡记录
            </p>
            <p v-if="!detail.deliveries.length">尚无交付成功证据，请核查闲管家，避免重复补发。</p>
          </div>
          <details class="business-card">
            <summary>采购尝试与经营记录</summary>
            <article v-for="p in detail.purchases" :key="p.id">
              <p>
                {{ p.state }} · {{ p.data.source?.data.shop }} · {{ p.data.purchaseNo }} · ¥{{
                  p.data.actualCost ?? '待核算'
                }}
              </p>
              <p>{{ p.data.note }} · {{ showTime(p.updatedAt) }}</p>
            </article>
            <p v-for="(e, i) in detail.events" :key="i">
              {{ showTime(e.createdAt) }} · {{ e.action }} · {{ e.data.note || e.data.error || '' }}
            </p>
          </details>
          <form class="business-form" @submit.prevent="saveFinance">
            <h3>订单核算</h3>
            <p class="hint">
              金额为元。没有成本或费用信息时留空，不按零成本计算毛利；实体成本取采购实付总额。
            </p>
            <label v-if="detail.kind === 'digital'" class="field"
              ><span>电子订单分摊成本</span
              ><input
                v-model.number="finance.cost"
                class="input"
                type="number"
                min="0"
                step="0.01" /></label
            ><label class="field"
              ><span>平台及其他费用</span
              ><input
                v-model.number="finance.fees"
                class="input"
                type="number"
                min="0"
                step="0.01" /></label
            ><label class="field"
              ><span>实际退款总额</span
              ><input
                v-model.number="finance.refund"
                class="input"
                type="number"
                min="0"
                step="0.01" /></label
            ><UiButton class="btn ghost" :disabled="busy">保存核算</UiButton>
          </form>
          <form class="business-form" @submit.prevent="saveNote">
            <h3>售后与处理记录</h3>
            <textarea
              v-model="note"
              class="input textarea"
              required
              placeholder="记录缺货、失效链接、采购取消、退款或核查结果"
            /><label
              v-if="['after_sale', 'canceled_review'].includes(detail.state)"
              class="xy-check"
              ><input v-model="closeCase" type="checkbox" />已核实销售及采购处理完成，结案</label
            ><UiButton class="btn ghost" :disabled="busy">保存处理记录</UiButton>
          </form>
        </div>
      </div>
    </template>
    <template v-if="view === 'reports' && overview"
      ><p class="hint">
        成交按订单实付统计；贡献毛利扣除采购或分摊成本、费用与实际退款。缺少核算数据的订单不计入毛利。
      </p>
      <div class="business-metrics">
        <article v-for="(m, kind) in overview.metrics" :key="kind" class="business-card">
          <h3>{{ kindName(kind) }}</h3>
          <p>
            成交金额 <strong>{{ money(m.revenue) }}</strong>
          </p>
          <p>
            已核算贡献毛利 <strong>{{ money(m.margin) }}</strong>
          </p>
          <p>
            已录成本 {{ money(m.cost) }} · 费用 {{ money(m.fees) }} · 退款 {{ money(m.refund) }}
          </p>
          <p>
            待核算 <strong>{{ m.pendingCost }} 笔</strong>
          </p>
        </article>
      </div>
      <UiButton class="btn" @click="emit('navigate', 'fulfillment')">
        前往订单补全核算
      </UiButton></template
    >
  </section>
</template>
