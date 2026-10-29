<script setup>
import { computed, ref, reactive, watch, onMounted, onBeforeUnmount } from "vue";
import { api, openBrowser, taobaoSourceUrl } from "../api";
import BookSourceAi from "./redesign/BookSourceAi.vue";
import {
  emptySource,
  showTime,
  kindName,
  fulfillmentStates,
} from "../commerce";
const props = defineProps({
  bookId: Number,
  section: { type: String, default: "sources" },
});
const emit = defineEmits(["updated", "open-fulfillment", "ai-settings", "ai-progress"]);
const sourceAi = ref(null);
const data = ref(null),
  tab = ref("sources"),
  busy = ref(false),
  error = ref(""),
  message = ref("");
const source = reactive({ id: null, primary: false, data: emptySource() });
const sourceDialog = ref(false);
function closeSource() { if (!busy.value) sourceDialog.value = false; }
const resource = reactive({
  version: "",
  url: "",
  code: "",
  instructions: "",
  edition: "",
  hostingRef: "",
});
const profile = reactive({ publicationYear: "", volume: "", confirmed: false });
const channel = reactive({
  shop: "",
  data: {
    mode: "physical",
    title: "",
    content: "",
    physicalPrice: null,
    digitalPrice: null,
    physicalStock: 1,
    digitalStock: 100,
    resourceId: null,
  },
});
const shops = ref([]),
  selectedChannel = ref(null),
  remote = ref(null),
  mapping = reactive({
    productId: "",
    physicalSkuId: "",
    digitalSkuId: "",
    confirmed: false,
  }),
  check = ref(null);
const candidates = ref([]);
const priceRun = ref(null), priceBusy = ref(false), priceError = ref(''), priceRefreshError = ref(''), priceOrder = ref('total');
let priceTimer, priceVersion = 0;
const priceStates = {searching:'采集中',waiting:'待继续',completed:'已完成',canceled:'已停止',idle:'未开始'};
const priceRows = computed(() => [...(priceRun.value?.rows || [])].sort((a,b) => {
  const key = priceOrder.value;
  const av = a[key] ?? Infinity, bv = b[key] ?? Infinity;
  return av-bv || (a.displayPrice ?? Infinity)-(b.displayPrice ?? Infinity) || a.source.id-b.source.id;
}));
const otherSources = computed(() => (data.value?.sources || []).filter(s => !priceRun.value || priceRun.value.state==='idle' || !priceRows.value.some(r=>r.source.id===s.id)));
const yuan = v => v == null ? '—' : '¥'+Number(v).toFixed(2);
async function loadPrice() {
  const run = ++priceVersion, id = props.bookId;
  clearTimeout(priceTimer);
  try {
    const result = await api.commerce(`/books/${id}/price-search`);
    if (run !== priceVersion || id !== props.bookId) return;
    priceRefreshError.value = '';
    priceRun.value = result;
    if (result.state==='searching') priceTimer = setTimeout(() => document.hidden ? loadPriceLater() : loadPrice(),2000);
  } catch (e) { if (run===priceVersion) { priceRefreshError.value = e.message; priceTimer = setTimeout(loadPrice,5000); } }
}
function loadPriceLater() { priceTimer = setTimeout(loadPrice,4000); }
async function priceAction(action) {
  if (priceBusy.value) return;
  priceBusy.value = true; priceError.value = '';
  const id = props.bookId;
  try {
    const result = await api.commerce(`/books/${id}/price-search`, {action,requestId:priceRun.value?.requestId,analyze:action==='cancel' && !!priceRun.value?.aiRequest && priceRun.value?.count>0});
    if (id !== props.bookId) return;
    priceRun.value = result;
    if (action==='start' || action==='resume') await openBrowser('taobao',result.searchUrl);
    await loadPrice();
    await load();
  } catch (e) { priceError.value = e.message; }
  finally { priceBusy.value = false; }
}
async function aiSearchStarted(result) {
  priceRun.value = result;
  try { await openBrowser('taobao', result.searchUrl); }
  catch (e) { priceError.value = e.message; }
  await loadPrice();
}
async function refreshSources() {
  if (tab.value !== 'sources') return;
  await load();
  await loadCandidates();
  await loadPrice();
  emit('updated');
}
async function searchTaobao() {
  error.value = '';
  try { await openBrowser('taobao', taobaoSourceUrl(data.value)); }
  catch (e) { error.value = e.message; }
}
async function openSource(url) {
  try {
    const target = new URL(url);
    target.searchParams.set('taoshuSourceBook', String(props.bookId));
    await openBrowser('taobao', target.href);
  } catch (e) { error.value = e.message; }
}
onMounted(() => window.addEventListener('focus', refreshSources));
onBeforeUnmount(() => { window.removeEventListener('focus', refreshSources); clearTimeout(priceTimer); priceVersion++; });
async function loadCandidates() {
  try {
    candidates.value = (await api.commerce("/sources")).filter(
      (s) => !s.bookId,
    );
  } catch (e) {
    error.value = e.message;
  }
}
watch(
  () => props.section,
  (v) => {
    tab.value = v;
    if (v === "channels") loadShops();
    if (v === "sources") loadCandidates();
  },
  { immediate: true },
);
defineExpose({
  refresh: async () => {
    await load();
    if (tab.value === "sources") await loadCandidates();
    if (tab.value === "sources") await loadPrice();
  },
});
const margin = ref(30);
const suggestedPrice = computed(() =>
  typeof source.data.price === "number" &&
  typeof source.data.freight === "number" &&
  margin.value >= 0 &&
  margin.value < 100
    ? Math.ceil(
        (source.data.price + source.data.freight) / (1 - margin.value / 100),
      )
    : null,
);
let version = 0;
async function load() {
  const run = ++version;
  try {
    const result = await api.commerce("/books/" + props.bookId);
    if (run !== version) return;
    data.value = result;
    await sourceAi.value?.refresh();
    Object.assign(
      profile,
      { publicationYear: "", volume: "", confirmed: false },
      result.profile,
    );
    if (!channel.data.title) channel.data.title = result.title;
  } catch (e) {
    if (run === version) error.value = e.message;
  }
}
watch(
  () => props.bookId,
  () => {
    data.value = null;
    error.value = "";
    message.value = "";
    selectedChannel.value = null;
    remote.value = null;
    check.value = null;
    source.id = null;
    source.primary = false;
    source.data = emptySource();
    sourceDialog.value = false;
    priceRun.value = null;
    priceError.value = '';
    priceRefreshError.value = '';
    clearTimeout(priceTimer);
    Object.assign(resource, {
      version: "",
      url: "",
      code: "",
      instructions: "",
      edition: "",
      hostingRef: "",
    });
    channel.shop = "";
    channel.data = {
      mode: "physical",
      title: "",
      content: "",
      physicalPrice: null,
      digitalPrice: null,
      physicalStock: 1,
      digitalStock: 100,
      resourceId: null,
    };
    load();
    if (props.section==='sources') loadPrice();
  },
  { immediate: true },
);
async function act(work) {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  message.value = "";
  try {
    await work();
    await load();
    emit("updated");
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
async function saveSource() {
  await act(async () => {
    const d = {
      ...source.data,
      price: source.data.price === "" ? null : source.data.price,
      freight: source.data.freight === "" ? null : source.data.freight,
    };
    await api.commerce("/sources", {
      id: source.id,
      bookId: props.bookId,
      primary: source.primary,
      data: d,
    });
    sourceDialog.value = false;
    await loadCandidates();
    await loadPrice();
    source.id = null;
    source.primary = false;
    source.data = emptySource();
    message.value = "货源已保存";
  });
}
function editSource(s = null, primary = false) {
  error.value = "";
  message.value = "";
  source.id = s?.id ?? null;
  source.primary = primary || !!s?.primary;
  source.data = { ...emptySource(), ...s?.data };
  sourceDialog.value = true;
}
async function addResource() {
  await act(async () => {
    await api.commerce("/books/" + props.bookId + "/resources", resource);
    Object.assign(resource, {
      version: "",
      url: "",
      code: "",
      instructions: "",
      edition: "",
      hostingRef: "",
    });
    message.value = "资源版本已入库，请核验后设为可用";
  });
}
async function setResource(r, state) {
  if (
    state === "ready" &&
    !window.confirm("已核对网盘链接可访问、内容完整，且与本书版次一致？")
  )
    return;
  await act(() =>
    api.commerce("/resources/" + r.id + "/state", {
      state,
      checked: state === "ready",
      note: state === "invalid" ? "链接或内容失效" : "",
    }),
  );
}
async function loadShops() {
  try {
    shops.value = (await api.xianyuCall("shops")).list || [];
  } catch (e) {
    error.value = e.message;
  }
}
async function saveChannel() {
  await act(async () => {
    const result = await api.commerce("/channels", {
      bookId: props.bookId,
      shop: channel.shop,
      data: channel.data,
    });
    check.value = result.check;
    message.value = "已保存渠道草稿";
  });
}
function editChannel(c) {
  selectedChannel.value = c;
  channel.shop = c.shop;
  channel.data = { ...channel.data, ...c.data };
  mapping.productId = c.productId;
  mapping.physicalSkuId = c.data.physicalSkuId || "";
  mapping.digitalSkuId = c.data.digitalSkuId || "";
  mapping.confirmed = false;
  remote.value = null;
  check.value = null;
}
async function inspectChannel(c) {
  editChannel(c);
  await act(async () => {
    check.value = await api.commerce("/channels/" + c.id + "/check");
    if (c.productId)
      remote.value = await api.commerce("/channels/" + c.id + "/remote", {
        productId: c.productId,
      });
  });
}
async function inspectRemote() {
  await act(async () => {
    remote.value = null;
    remote.value = await api.commerce(
      "/channels/" + selectedChannel.value.id + "/remote",
      {
        productId: mapping.productId,
      },
    );
  });
}
async function saveMap() {
  await act(async () => {
    await api.commerce(
      "/channels/" + selectedChannel.value.id + "/map",
      mapping,
    );
    message.value = "规格映射已核实保存；这不代表自动发货能力已验证";
  });
}
</script>

<template>
  <section class="business-editor">
    <p v-if="error" role="alert" class="xy-error">{{ error }}</p>
    <p v-if="message" role="status" class="hint">{{ message }}</p>
    <p v-if="!data" class="hint">正在加载经营资料…</p>
    <template v-else>
      <form
        v-if="tab === 'profile'"
        @submit.prevent="
          act(() => api.commerce('/books/' + bookId + '/profile', profile))
        "
        class="business-form"
      >
        <label class="field"
          ><span>出版年份</span
          ><input
            v-model="profile.publicationYear"
            class="input"
            placeholder="例如 2023"
        /></label>
        <label class="field"
          ><span>册次</span
          ><input
            v-model="profile.volume"
            class="input"
            placeholder="例如 上册 / 下册 / 全一册"
        /></label>
        <label class="xy-check"
          ><input
            type="checkbox"
            v-model="profile.confirmed"
          />已核对书目身份、版次和册次</label
        ><UiButton class="btn" :disabled="busy">保存确认</UiButton>
      </form>
      <div v-if="tab === 'schools'">
        <p v-if="!data.schoolRecords.length" class="hint">暂无学校用书关联。</p>
        <article
          v-for="r in data.schoolRecords"
          :key="r.id"
          class="business-card"
        >
          <strong>{{ r.data.school }} · {{ r.data.year }}</strong>
          <p>
            {{ r.data.college }} / {{ r.data.major }} / {{ r.data.stage }} /
            {{ r.data.subject }}
          </p>
          <p>{{ r.data.raw }}</p>
          <a
            v-if="/^https?:\/\//.test(r.data.sourceUrl)"
            :href="r.data.sourceUrl"
            target="_blank"
            rel="noreferrer"
            >查看书目出处</a
          >
        </article>
      </div>
      <div v-if="tab === 'orders'">
        <p v-if="!data.fulfillments?.length" class="hint">
          暂无已准确关联规格的履约订单。
        </p>
        <article
          v-for="f in data.fulfillments"
          :key="f.id"
          class="business-card"
        >
          <strong>{{ f.orderNo }} · {{ kindName(f.kind) }}</strong>
          <p>
            {{ fulfillmentStates[f.state] }} · 实付 ¥{{
              ((f.snapshot.pay_amount || 0) / 100).toFixed(2)
            }}
          </p>
          <UiButton class="btn tiny" @click="emit('open-fulfillment', f.id)">
            采购 / 交付 / 售后 / 核算详情
          </UiButton>
        </article>
      </div>
      <div v-if="tab === 'sources'">
        <BookSourceAi ref="sourceAi" :book-id="bookId" :search="priceRun" @updated="refreshSources" @edit-source="editSource" @add-source="editSource()" @search-started="aiSearchStarted" @settings="emit('ai-settings')" @progress="emit('ai-progress',$event)" />
        <div class="module-bar source-records-bar"><h3>货源资料</h3><div class="row">
        <UiButton class="btn small" :disabled="priceBusy || ['searching','waiting'].includes(priceRun?.state)" @click="priceAction('start')">{{ priceBusy ? '处理中…' : '仅搜索比价' }}</UiButton>
        <UiButton
          class="btn ghost"
          @click="searchTaobao"
        >
          淘宝找货源
        </UiButton>
        <UiButton class="btn ghost" @click="refreshSources">刷新货源</UiButton>
        </div></div>
        <p v-if="priceError" class="error-panel" role="alert">{{ priceError }}</p>
        <p v-if="priceRefreshError" class="error-panel" role="alert">{{ priceRefreshError }}</p>
        <section v-if="priceRun && priceRun.state!=='idle'" class="price-comparison">
          <div class="module-bar">
            <div class="row"><strong>{{ priceStates[priceRun.state] }}</strong><span class="muted">{{ priceRun.count }} 个候选 · 滚动采集</span></div>
            <div class="row">
              <UiButton v-if="priceRun.state==='waiting'" class="btn small" :disabled="priceBusy" @click="priceAction('resume')">继续检索</UiButton>
              <UiButton v-if="['searching','waiting'].includes(priceRun.state)" class="btn ghost small" :disabled="priceBusy" @click="priceAction('cancel')">{{ priceRun.aiRequest && priceRun.count ? '停止并整理' : '停止' }}</UiButton>
              <UiButton class="btn ghost small" @click="loadPrice">刷新比价</UiButton>
            </div>
          </div>
          <p v-if="priceRun.error" class="error-panel" role="alert">{{ priceRun.error }}</p>
          <div class="price-comparison-summary">
            <strong>{{ priceRun.bestVerified ? '最低已核验含运费价 '+yuan(priceRun.bestVerified.total) : '暂无已核验含运费价' }}</strong>
            <span v-if="priceRun.bestDisplay" class="muted">最低展示价 {{ yuan(priceRun.bestDisplay.displayPrice) }} · 仅供筛选</span>
            <SearchSelect v-model="priceOrder" class="input narrow" aria-label="比价排序"><option value="total">已核验含运费价优先</option><option value="displayPrice">展示价从低到高</option></SearchSelect>
          </div>
          <div v-if="priceRows.length" class="price-comparison-table">
            <table class="data-table"><thead><tr><th>货源 / 规格</th><th class="numeric">展示价</th><th class="numeric">已核验含运费</th><th>核验状态</th><th></th></tr></thead>
              <tbody><tr v-for="row in priceRows" :key="row.source.id">
                <td><strong>{{ row.title || row.source.data.title }}</strong><small>{{ row.source.data.shop || '店铺待核对' }} · {{ row.source.data.sku || '规格待核对' }}</small></td>
                <td class="numeric">{{ yuan(row.displayPrice) }}<small v-if="row.priceLabel">{{ row.priceLabel }}</small></td><td class="numeric">{{ yuan(row.total) }}</td>
                <td><span class="tag" :class="row.total==null?'warning':'success'">{{ row.reason }}</span><small v-if="row.total!=null">{{ showTime(row.source.data.checkedAt) }}</small></td>
                <td><UiButton class="btn small" @click="editSource(row.source)">{{ row.total==null?'核对':'编辑' }}</UiButton> <UiButton class="btn ghost small" @click="openSource(row.source.data.url)">采集详情</UiButton></td>
              </tr></tbody>
            </table>
          </div>
          <p v-else class="muted">等待淘宝返回候选；登录或验证后可继续检索。</p>
          <p class="muted price-comparison-scope">{{ priceRun.completionReason==='visible-end' ? '当前未再加载到新商品。' : '' }}范围：本次采集候选与已有货源；核验版本、规格、品相、运费和代发后计入含运费价。</p>
        </section>

        <details v-if="candidates.length" class="candidate-list">
          <summary>采集候选 · {{ candidates.length }}</summary>
          <div
            v-for="candidate in candidates"
            :key="candidate.id"
            class="candidate-row"
          >
            <span>{{ candidate.data.title }} · {{ candidate.data.shop }}</span
            ><UiButton class="btn small" @click="editSource(candidate)"
              >选择并核对</UiButton
            >
          </div>
        </details>
        <article v-for="s in otherSources" :key="s.id" class="business-card">
          <strong
            >{{ s.primary ? "主选 · " : ""
            }}{{ s.data.shop || s.data.title }}</strong
          >
          <p v-if="s.data.shop && s.data.title">{{ s.data.title }}</p>
          <p>
            {{ s.data.sku || "规格待核对" }} · ¥{{ s.data.price ?? "待核价" }} +
            运费 {{ s.data.freight ?? "待核价" }} ·
            {{
              s.data.status === "available"
                ? "已核验"
                : s.data.status === "unavailable"
                  ? "不可用"
                  : "待核验"
            }}
          </p>
          <p class="hint">{{ showTime(s.data.checkedAt) }}</p>
          <div class="head-actions">
            <UiButton class="link-btn" @click="editSource(s)">编辑</UiButton
            ><UiButton
              class="link-btn"
              @click="openSource(s.data.url)"
              >打开商品采集</UiButton
            >
          </div>
        </article>
        <Teleport to="body">
          <div v-if="sourceDialog" class="modal-backdrop">
            <section class="modal-dialog source-editor" role="dialog" aria-modal="true" aria-labelledby="source-editor-title" v-modal-focus="closeSource">
              <header class="source-editor-header">
                <div><h2 id="source-editor-title">{{ source.id ? '编辑货源' : '新增货源' }}</h2><p class="muted">{{ data.title }}</p></div>
                <UiButton type="button" class="btn ghost" :disabled="busy" @click="closeSource">关闭</UiButton>
              </header>
              <form @submit.prevent="saveSource" class="source-editor-form">
                <div class="source-editor-body">
                  <div class="source-editor-grid">
                    <label class="field wide"><span>淘宝商品链接</span><input v-model="source.data.url" class="input" required /></label>
                    <label class="field"><span>店铺</span><input v-model="source.data.shop" class="input" /></label>
                    <label class="field source-spec"><span>规格 / ISBN / 版次</span><input v-model="source.data.sku" class="input" /></label>
                    <label class="field"><span>品相</span><input v-model="source.data.condition" class="input" /></label>
                    <label class="field"><span>采购单价（元）</span><input v-model.number="source.data.price" class="input" type="number" min="0" step="0.01" /></label>
                    <label class="field"><span>运费（元）</span><input v-model.number="source.data.freight" class="input" type="number" min="0" step="0.01" /></label>
                    <label class="field"><span>货源状态</span><SearchSelect v-model="source.data.status" class="input"><option value="pending">待核验</option><option value="available">可用</option><option value="unavailable">不可用</option></SearchSelect></label>
                    <label class="field"><span>目标毛利率（%）</span><input v-model.number="margin" class="input" type="number" min="0" max="99" /></label>
                    <div class="field"><span>参考售价</span><strong class="source-reference">{{ suggestedPrice === null ? '待补充成本' : '¥' + suggestedPrice }}</strong><small class="muted">未扣平台费用</small></div>
                  </div>
                  <div class="source-editor-checks">
                    <label class="check-line"><input v-model="source.data.dropship" type="checkbox" />支持代发</label>
                    <label class="check-line"><input v-model="source.primary" type="checkbox" />设为主选货源</label>
                    <label class="check-line"><input type="checkbox" :checked="!!source.data.checkedAt" @change="source.data.checkedAt = $event.target.checked ? new Date().toISOString() : ''" />本次已核验</label>
                  </div>
                  <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
                </div>
                <footer class="source-editor-footer">
                  <UiButton type="button" class="btn ghost" :disabled="busy" @click="closeSource">取消</UiButton>
                  <UiButton type="submit" class="btn primary" :disabled="busy">{{ busy ? '保存中…' : '保存货源' }}</UiButton>
                </footer>
              </form>
            </section>
          </div>
        </Teleport>
      </div>
      <div v-if="tab === 'resources'">
        <article v-for="r in data.resources" :key="r.id" class="business-card">
          <strong
            >{{ r.version }} ·
            {{
              {
                pending: "待核验",
                ready: "可用",
                invalid: "失效",
                retired: "停用",
              }[r.state]
            }}</strong
          >
          <p>
            <a :href="r.data.url" target="_blank" rel="noreferrer"
              >打开网盘资源</a
            >
            · 提取码
            {{ r.data.code || "无" }}
          </p>
          <p>{{ r.data.instructions }}</p>
          <p class="hint">
            托管配置：{{
              r.data.hostingRef || "尚未配置"
            }}（离线自动交付待验证）
          </p>
          <div class="head-actions">
            <UiButton
              class="btn tiny"
              :disabled="busy"
              @click="setResource(r, 'ready')"
            >
              核验可用</UiButton
            ><UiButton
              class="btn tiny ghost"
              :disabled="busy"
              @click="setResource(r, 'invalid')"
            >
              标记失效</UiButton
            ><UiButton
              class="btn tiny ghost"
              :disabled="busy"
              @click="setResource(r, 'retired')"
            >
              停用
            </UiButton>
          </div>
        </article>
        <form @submit.prevent="addResource" class="business-form">
          <h4>新增资源版本</h4>
          <label
            v-for="(label, key) in {
              version: '资源版本名称',
              url: '网盘链接',
              code: '提取码',
              edition: '对应版次',
              hostingRef: '闲管家托管配置备注',
            }"
            :key="key"
            class="field"
            ><span>{{ label }}</span
            ><input
              v-model="resource[key]"
              class="input"
              :required="['version', 'url'].includes(key)" /></label
          ><label class="field"
            ><span>交付说明</span
            ><textarea
              v-model="resource.instructions"
              class="input textarea"
              required
              rows="3"
            /></label
          ><UiButton class="btn" :disabled="busy">入库新版本</UiButton>
        </form>
      </div>
      <div v-if="tab === 'channels'">
        <article v-for="c in data.channels" :key="c.id" class="business-card">
          <strong
            >{{ c.shop }} ·
            {{
              c.data.mode === "mixed" ? "电子＋实体双规格" : "实体单规格"
            }}</strong
          >
          <p class="mono">{{ c.productId || "本地草稿，尚未发布" }}</p>
          <UiButton
            class="btn tiny"
            :disabled="busy"
            @click="inspectChannel(c)"
          >
            查看 / 校验 / 实时库存
          </UiButton>
        </article>
        <details class="detail-section">
          <summary>商品规格配置</summary>
          <form @submit.prevent="saveChannel" class="business-form">
            <h4>准备销售草稿</h4>
            <label class="field"
              ><span>闲鱼店铺</span
              ><SearchSelect v-model="channel.shop" class="input" required>
                <option value="">选择店铺</option>
                <option
                  v-for="s in shops"
                  :key="s.user_name"
                  :value="s.user_name"
                >
                  {{ s.shop_name || s.user_name }}
                </option>
              </SearchSelect></label
            ><label class="field"
              ><span>商品形态</span
              ><SearchSelect v-model="channel.data.mode" class="input">
                <option value="mixed">同商品：电子版＋实体版</option>
                <option value="physical">现有实体单规格商品</option>
              </SearchSelect></label
            ><label class="field"
              ><span>商品标题</span
              ><input
                v-model="channel.data.title"
                class="input"
                required /></label
            ><label class="field"
              ><span>商品描述</span
              ><textarea
                v-model="channel.data.content"
                class="input textarea"
                rows="3"
              />
            </label>
            <div class="form-grid two">
              <label class="field"
                ><span>实体售价（元）</span
                ><input
                  v-model.number="channel.data.physicalPrice"
                  class="input"
                  type="number"
                  min="0.01"
                  step="0.01" /></label
              ><label class="field"
                ><span>电子售价（元）</span
                ><input
                  v-model.number="channel.data.digitalPrice"
                  class="input"
                  type="number"
                  min="0.01"
                  step="0.01" /></label
              ><label class="field"
                ><span>实体可售量</span
                ><input
                  v-model.number="channel.data.physicalStock"
                  class="input"
                  type="number"
                  min="0"
                  max="9999" /></label
              ><label class="field"
                ><span>电子可售量</span
                ><input
                  v-model.number="channel.data.digitalStock"
                  class="input"
                  type="number"
                  min="0"
                  max="9999"
              /></label>
            </div>
            <label class="field"
              ><span>电子资源版本</span
              ><SearchSelect v-model="channel.data.resourceId" class="input">
                <option :value="null">未选择</option>
                <option v-for="r in data.resources" :key="r.id" :value="r.id">
                  {{ r.version }} · {{ r.state }}
                </option>
              </SearchSelect></label
            ><UiButton class="btn" :disabled="busy">保存草稿并校验</UiButton>
          </form>
        </details>
        <div v-if="check" class="business-card">
          <strong>{{ check.ready ? "资料校验通过" : "发布阻塞项" }}</strong>
          <ul>
            <li v-for="reason in check.blockers" :key="reason">{{ reason }}</li>
          </ul>
        </div>
        <form
          v-if="selectedChannel"
          @submit.prevent="saveMap"
          class="business-form"
        >
          <h4>绑定已存在的远端商品规格</h4>
          <p class="hint">自动交付待验证。</p>
          <label class="field"
            ><span>管家商品 ID</span
            ><input
              v-model="mapping.productId"
              class="input mono"
              required /></label
          ><UiButton
            type="button"
            class="btn ghost"
            :disabled="busy"
            @click="inspectRemote"
          >
            读取商品及规格
          </UiButton>
          <template v-if="remote"
            ><p>
              商品总库存：{{ remote.stock ?? "未知" }} ·
              {{ showTime(remote.checkedAt) }}
            </p>
            <p v-if="remote.skuError" class="xy-error">{{ remote.skuError }}</p>
            <p v-for="sku in remote.skuItems" :key="sku.sku_id">
              {{ sku.sku_text }} · 库存 {{ sku.stock }} · ID {{ sku.sku_id }}
            </p>
            <label class="field"
              ><span>实体规格 ID（单规格填 0）</span
              ><input
                v-model="mapping.physicalSkuId"
                class="input mono"
                required /></label
            ><label v-if="channel.data.mode === 'mixed'" class="field"
              ><span>电子规格 ID</span
              ><input
                v-model="mapping.digitalSkuId"
                class="input mono"
                required /></label
            ><label class="xy-check"
              ><input
                v-model="mapping.confirmed"
                type="checkbox"
                required
              />已核对规格文字和售卖形态</label
            ><UiButton class="btn" :disabled="busy"
              >保存规格关联</UiButton
            ></template
          >
        </form>
      </div>
    </template>
  </section>
</template>
