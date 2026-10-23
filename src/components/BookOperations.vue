<script setup>
import { computed, ref, reactive, watch } from 'vue';
import { api, openBrowser } from '../api';
import { emptySource, showTime, kindName, fulfillmentStates } from '../commerce';
const props = defineProps({ bookId: Number });
const emit = defineEmits(['updated', 'open-fulfillment']);
const data = ref(null),
  tab = ref('sources'),
  busy = ref(false),
  error = ref(''),
  message = ref('');
const source = reactive({ id: null, primary: false, data: emptySource() });
const resource = reactive({
  version: '',
  url: '',
  code: '',
  instructions: '',
  edition: '',
  hostingRef: '',
});
const profile = reactive({ publicationYear: '', volume: '', confirmed: false });
const channel = reactive({
  shop: '',
  data: {
    mode: 'mixed',
    title: '',
    content: '',
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
  mapping = reactive({ productId: '', physicalSkuId: '', digitalSkuId: '', confirmed: false }),
  check = ref(null);
const margin = ref(30);
const suggestedPrice = computed(() =>
  typeof source.data.price === 'number' &&
  typeof source.data.freight === 'number' &&
  margin.value >= 0 &&
  margin.value < 100
    ? Math.ceil((source.data.price + source.data.freight) / (1 - margin.value / 100))
    : null,
);
let version = 0;
async function load() {
  const run = ++version;
  try {
    const result = await api.commerce('/books/' + props.bookId);
    if (run !== version) return;
    data.value = result;
    Object.assign(profile, { publicationYear: '', volume: '', confirmed: false }, result.profile);
    if (!channel.data.title) channel.data.title = result.title;
  } catch (e) {
    if (run === version) error.value = e.message;
  }
}
watch(
  () => props.bookId,
  () => {
    data.value = null;
    error.value = '';
    message.value = '';
    selectedChannel.value = null;
    remote.value = null;
    check.value = null;
    source.id = null;
    source.primary = false;
    source.data = emptySource();
    Object.assign(resource, {
      version: '',
      url: '',
      code: '',
      instructions: '',
      edition: '',
      hostingRef: '',
    });
    channel.shop = '';
    channel.data = {
      mode: 'mixed',
      title: '',
      content: '',
      physicalPrice: null,
      digitalPrice: null,
      physicalStock: 1,
      digitalStock: 100,
      resourceId: null,
    };
    load();
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
    await load();
    emit('updated');
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
      price: source.data.price === '' ? null : source.data.price,
      freight: source.data.freight === '' ? null : source.data.freight,
    };
    await api.commerce('/sources', {
      id: source.id,
      bookId: props.bookId,
      primary: source.primary,
      data: d,
    });
    source.id = null;
    source.primary = false;
    source.data = emptySource();
    message.value = '货源已保存';
  });
}
function editSource(s) {
  source.id = s.id;
  source.primary = s.primary;
  source.data = { ...emptySource(), ...s.data };
}
async function addResource() {
  await act(async () => {
    await api.commerce('/books/' + props.bookId + '/resources', resource);
    Object.assign(resource, {
      version: '',
      url: '',
      code: '',
      instructions: '',
      edition: '',
      hostingRef: '',
    });
    message.value = '资源版本已入库，请核验后设为可用';
  });
}
async function setResource(r, state) {
  if (state === 'ready' && !window.confirm('已核对网盘链接可访问、内容完整，且与本书版次一致？'))
    return;
  await act(() =>
    api.commerce('/resources/' + r.id + '/state', {
      state,
      checked: state === 'ready',
      note: state === 'invalid' ? '链接或内容失效' : '',
    }),
  );
}
async function loadShops() {
  try {
    shops.value = (await api.xianyuCall('shops')).list || [];
  } catch (e) {
    error.value = e.message;
  }
}
async function saveChannel() {
  await act(async () => {
    const result = await api.commerce('/channels', {
      bookId: props.bookId,
      shop: channel.shop,
      data: channel.data,
    });
    check.value = result.check;
    message.value = '已保存渠道草稿';
  });
}
function editChannel(c) {
  selectedChannel.value = c;
  channel.shop = c.shop;
  channel.data = { ...channel.data, ...c.data };
  mapping.productId = c.productId;
  mapping.physicalSkuId = c.data.physicalSkuId || '';
  mapping.digitalSkuId = c.data.digitalSkuId || '';
  mapping.confirmed = false;
  remote.value = null;
  check.value = null;
}
async function inspectChannel(c) {
  editChannel(c);
  await act(async () => {
    check.value = await api.commerce('/channels/' + c.id + '/check');
    if (c.productId)
      remote.value = await api.commerce('/channels/' + c.id + '/remote', {
        productId: c.productId,
      });
  });
}
async function inspectRemote() {
  await act(async () => {
    remote.value = null;
    remote.value = await api.commerce('/channels/' + selectedChannel.value.id + '/remote', {
      productId: mapping.productId,
    });
  });
}
async function saveMap() {
  await act(async () => {
    await api.commerce('/channels/' + selectedChannel.value.id + '/map', mapping);
    message.value = '规格映射已核实保存；这不代表自动发货能力已验证';
  });
}
</script>

<template>
  <section class="block business-book">
    <div class="block-head">
      <h3>图书经营</h3>
      <button class="link-btn" :disabled="busy" @click="load">刷新经营资料</button>
    </div>
    <div class="business-tabs">
      <button
        v-for="(name, key) in {
          profile: '版本确认',
          schools: '适用学校',
          sources: '实体货源',
          resources: '电子资源',
          channels: '销售商品',
          orders: '订单与经营记录',
        }"
        :key="key"
        class="btn tiny"
        :class="{ accent: tab === key }"
        @click="
          tab = key;
          key === 'channels' && loadShops();
        "
      >
        {{ name }}
      </button>
    </div>
    <p v-if="error" role="alert" class="xy-error">{{ error }}</p>
    <p v-if="message" role="status" class="hint">{{ message }}</p>
    <p v-if="!data" class="hint">正在加载经营资料…</p>
    <template v-else>
      <form
        v-if="tab === 'profile'"
        @submit.prevent="act(() => api.commerce('/books/' + bookId + '/profile', profile))"
        class="business-form"
      >
        <p class="hint">ISBN 和版次在上方基本信息中维护；出版年份和册次用于区分同名不同版图书。</p>
        <label class="field"
          ><span>出版年份</span
          ><input v-model="profile.publicationYear" class="input" placeholder="例如 2023"
        /></label>
        <label class="field"
          ><span>册次</span
          ><input v-model="profile.volume" class="input" placeholder="例如 上册 / 下册 / 全一册"
        /></label>
        <label class="xy-check"
          ><input type="checkbox" v-model="profile.confirmed" />已核对书目身份、版次和册次</label
        ><button class="btn" :disabled="busy">保存确认</button>
      </form>
      <div v-if="tab === 'schools'">
        <p v-if="!data.schoolRecords.length" class="hint">
          尚无学校用书关联。前往学校书单导入并确认。
        </p>
        <article v-for="r in data.schoolRecords" :key="r.id" class="business-card">
          <strong>{{ r.data.school }} · {{ r.data.year }}</strong>
          <p>
            {{ r.data.college }} / {{ r.data.major }} / {{ r.data.stage }} / {{ r.data.subject }}
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
        <p v-if="!data.fulfillments?.length" class="hint">暂无已准确关联规格的履约订单。</p>
        <article v-for="f in data.fulfillments" :key="f.id" class="business-card">
          <strong>{{ f.orderNo }} · {{ kindName(f.kind) }}</strong>
          <p>
            {{ fulfillmentStates[f.state] }} · 实付 ¥{{
              ((f.snapshot.pay_amount || 0) / 100).toFixed(2)
            }}
          </p>
          <button class="btn tiny" @click="emit('open-fulfillment', f.id)">
            采购 / 交付 / 售后 / 核算详情
          </button>
        </article>
      </div>
      <div v-if="tab === 'sources'">
        <button
          class="btn ghost"
          @click="
            openBrowser(
              'taobao',
              'https://s.taobao.com/search?q=' + encodeURIComponent(data.isbn || data.title),
            )
          "
        >
          搜索本书实体货源
        </button>
        <p class="hint">一本书可有多个淘宝货源。采购价与货源可供量不等于本地库存。</p>
        <article v-for="s in data.sources" :key="s.id" class="business-card">
          <strong>{{ s.primary ? '主选 · ' : '' }}{{ s.data.shop || s.data.title }}</strong>
          <p>
            {{ s.data.sku || '规格待核对' }} · ¥{{ s.data.price ?? '待核价' }} + 运费
            {{ s.data.freight ?? '待核价' }} ·
            {{
              s.data.status === 'available'
                ? '已核验'
                : s.data.status === 'unavailable'
                  ? '不可用'
                  : '待核验'
            }}
          </p>
          <p class="hint">{{ showTime(s.data.checkedAt) }}</p>
          <div class="head-actions">
            <button class="link-btn" @click="editSource(s)">编辑</button
            ><button class="link-btn" @click="openBrowser('taobao', s.data.url)">打开货源</button>
          </div>
        </article>
        <form @submit.prevent="saveSource" class="business-form">
          <h4>{{ source.id ? '编辑货源' : '新增实体货源' }}</h4>
          <label
            v-for="(label, key) in {
              url: '淘宝商品链接',
              shop: '店铺',
              sku: '规格 / ISBN / 版次',
              condition: '品相',
            }"
            :key="key"
            class="field"
            ><span>{{ label }}</span
            ><input v-model="source.data[key]" class="input" :required="key === 'url'"
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
          <label class="field"
            ><span>目标毛利率（%）</span
            ><input v-model.number="margin" class="input" type="number" min="0" max="99"
          /></label>
          <p class="hint">
            参考售价：{{
              suggestedPrice === null ? '先填写采购价和运费' : '¥' + suggestedPrice
            }}（未扣平台费用）
          </p>
          <label class="xy-check"
            ><input v-model="source.data.dropship" type="checkbox" />支持代发</label
          ><label class="xy-check"
            ><input v-model="source.primary" type="checkbox" />设为主选货源</label
          >
          <label class="field"
            ><span>货源状态</span
            ><select v-model="source.data.status" class="input">
              <option value="pending">待核验</option>
              <option value="available">可用</option>
              <option value="unavailable">不可用</option>
            </select></label
          >
          <div class="head-actions">
            <button
              type="button"
              class="btn ghost"
              @click="source.data.checkedAt = new Date().toISOString()"
            >
              标记本次已核验</button
            ><button class="btn" :disabled="busy">保存货源</button>
          </div>
        </form>
      </div>
      <div v-if="tab === 'resources'">
        <p class="hint">同一链接可重复交付。修改链接或提取码时新增版本，历史交付内容保持可追溯。</p>
        <article v-for="r in data.resources" :key="r.id" class="business-card">
          <strong
            >{{ r.version }} ·
            {{
              { pending: '待核验', ready: '可用', invalid: '失效', retired: '停用' }[r.state]
            }}</strong
          >
          <p>
            <a :href="r.data.url" target="_blank" rel="noreferrer">打开网盘资源</a> · 提取码
            {{ r.data.code || '无' }}
          </p>
          <p>{{ r.data.instructions }}</p>
          <p class="hint">托管配置：{{ r.data.hostingRef || '尚未配置' }}（离线自动交付待验证）</p>
          <div class="head-actions">
            <button class="btn tiny" :disabled="busy" @click="setResource(r, 'ready')">
              核验可用</button
            ><button class="btn tiny ghost" :disabled="busy" @click="setResource(r, 'invalid')">
              标记失效</button
            ><button class="btn tiny ghost" :disabled="busy" @click="setResource(r, 'retired')">
              停用
            </button>
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
          ><button class="btn" :disabled="busy">入库新版本</button>
        </form>
      </div>
      <div v-if="tab === 'channels'">
        <article v-for="c in data.channels" :key="c.id" class="business-card">
          <strong
            >{{ c.shop }} ·
            {{ c.data.mode === 'mixed' ? '电子＋实体双规格' : '实体单规格' }}</strong
          >
          <p class="mono">{{ c.productId || '本地草稿，尚未发布' }}</p>
          <button class="btn tiny" :disabled="busy" @click="inspectChannel(c)">
            查看 / 校验 / 实时库存
          </button>
        </article>
        <form @submit.prevent="saveChannel" class="business-form">
          <h4>准备销售草稿</h4>
          <label class="field"
            ><span>闲鱼店铺</span
            ><select v-model="channel.shop" class="input" required>
              <option value="">选择店铺</option>
              <option v-for="s in shops" :key="s.user_name" :value="s.user_name">
                {{ s.shop_name || s.user_name }}
              </option>
            </select></label
          ><label class="field"
            ><span>商品形态</span
            ><select v-model="channel.data.mode" class="input">
              <option value="mixed">同商品：电子版＋实体版</option>
              <option value="physical">现有实体单规格商品</option>
            </select></label
          ><label class="field"
            ><span>商品标题</span
            ><input v-model="channel.data.title" class="input" required /></label
          ><label class="field"
            ><span>商品描述</span
            ><textarea v-model="channel.data.content" class="input textarea" rows="3" />
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
            ><select v-model="channel.data.resourceId" class="input">
              <option :value="null">未选择</option>
              <option v-for="r in data.resources" :key="r.id" :value="r.id">
                {{ r.version }} · {{ r.state }}
              </option>
            </select></label
          ><button class="btn" :disabled="busy">保存草稿并校验</button>
        </form>
        <div v-if="check" class="business-card">
          <strong>{{ check.ready ? '资料校验通过' : '发布阻塞项' }}</strong>
          <ul>
            <li v-for="reason in check.blockers" :key="reason">{{ reason }}</li>
          </ul>
        </div>
        <form v-if="selectedChannel" @submit.prevent="saveMap" class="business-form">
          <h4>绑定已存在的远端商品规格</h4>
          <p class="hint">仅核对与绑定现有商品，不会创建另一条商品。自动交付能力需另行验证。</p>
          <label class="field"
            ><span>管家商品 ID</span
            ><input v-model="mapping.productId" class="input mono" required /></label
          ><button type="button" class="btn ghost" :disabled="busy" @click="inspectRemote">
            读取商品及规格
          </button>
          <template v-if="remote"
            ><p>商品总库存：{{ remote.stock ?? '未知' }} · {{ showTime(remote.checkedAt) }}</p>
            <p v-if="remote.skuError" class="xy-error">{{ remote.skuError }}</p>
            <p v-for="sku in remote.skuItems" :key="sku.sku_id">
              {{ sku.sku_text }} · 库存 {{ sku.stock }} · ID {{ sku.sku_id }}
            </p>
            <label class="field"
              ><span>实体规格 ID（单规格填 0）</span
              ><input v-model="mapping.physicalSkuId" class="input mono" required /></label
            ><label v-if="channel.data.mode === 'mixed'" class="field"
              ><span>电子规格 ID</span
              ><input v-model="mapping.digitalSkuId" class="input mono" required /></label
            ><label class="xy-check"
              ><input
                v-model="mapping.confirmed"
                type="checkbox"
                required
              />已核对规格文字和售卖形态</label
            ><button class="btn" :disabled="busy">保存规格关联</button></template
          >
        </form>
      </div>
    </template>
  </section>
</template>
