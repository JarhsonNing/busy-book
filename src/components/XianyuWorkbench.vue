<script setup>
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { api } from '../api';
import { useToast } from '../composables/toast';
import {
  cents,
  date,
  imageModes,
  itemUrl,
  listOf,
  orderStates,
  productStates,
  taskStates,
  yuan,
  weightedLength,
} from '../xianyu';
const emit = defineEmits(['publish', 'updated', 'open-book']);
const toast = useToast();
const PAGE_SIZE = 100;
const tabs = {
  shops: '店铺',
  products: '商品管理',
  orders: '订单管理',
  tasks: '发布记录',
  config: '接入设置',
};
const tab = ref('shops'),
  busy = ref(false),
  error = ref(''),
  connected = ref(false),
  configured = ref(false);
const credentials = reactive({ appKey: '', appSecret: '' });
const hosting = reactive({ repoUrl: '', token: '', branch: '', directory: 'taoshu-images' });
const hostingConfigured = ref(false),
  hostingResult = ref('');
const shops = ref([]),
  rows = ref([]),
  tasks = ref([]),
  count = ref(0),
  page = ref(1),
  status = ref(''),
  shopFilter = ref(''),
  keyword = ref(''),
  lookup = ref('');
const days = ref(30),
  detail = ref(null),
  detailType = ref(''),
  companies = ref([]);
const editShop = ref(''),
  editor = reactive({ title: '', content: '', price: '', stock: 0 });
const shipping = reactive({
  waybill_no: '',
  express_code: '',
  express_name: '',
  customSender: false,
  ship_name: '',
  ship_mobile: '',
  ship_district_id: '',
  ship_address: '',
});
const lastUpdated = ref('');
let timer;
let finishInit;
const initialized = new Promise((resolve) => {
  finishInit = resolve;
});
const localBooks = ref([]),
  localQuery = ref(''),
  orderBookId = ref('');
async function findLocalBooks() {
  localBooks.value = await api.books({ q: localQuery.value, limit: 100 });
}
async function saveOrderBook(clear = false) {
  await action(async () => {
    if (!clear && !orderBookId.value) throw new Error('请选择本地图书');
    detail.value = await api.linkXianyuOrder(
      detail.value.order_no,
      clear ? null : Number(orderBookId.value),
    );
    orderBookId.value = detail.value.localBook?.id || '';
    rows.value = rows.value.map((row) =>
      row.order_no === detail.value.order_no ? detail.value : row,
    );
    toast.success(clear ? '已解除本地关联' : '已关联本地图书');
    emit('updated');
  });
}
function openLocalBook(book) {
  detail.value = null;
  emit('open-book', book.id);
}
async function openOrder(orderNo) {
  await initialized;
  tab.value = 'orders';
  await viewOrder(orderNo);
}
const visibleRows = computed(() =>
  rows.value.filter(
    (r) =>
      !keyword.value ||
      JSON.stringify([
        r.title,
        r.goods?.title,
        r.product_id,
        r.order_no,
        r.outer_id,
        r.localBook?.title,
        r.localBook?.isbn,
      ])
        .toLowerCase()
        .includes(keyword.value.toLowerCase()),
  ),
);
const totalPages = computed(() => Math.max(1, Math.min(100, Math.ceil(count.value / PAGE_SIZE))));
const productShops = computed(() => detail.value?.publish_shop || []);
const singleSpec = computed(
  () => !detail.value?.sku_items?.length && Number(detail.value?.spec_type || 1) === 1,
);
const pendingTasks = computed(() =>
  tasks.value.filter((t) => ['publishing', 'preparing'].includes(t.status)),
);
async function action(work) {
  if (busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    await work();
  } catch (e) {
    error.value = e.message;
    toast.error(e.message);
  } finally {
    busy.value = false;
  }
}
async function loadShops() {
  shops.value = listOf(await api.xianyuCall('shops'));
  connected.value = true;
}
async function loadData() {
  if (!configured.value || tab.value === 'config') return;
  if (tab.value === 'shops') await loadShops();
  else if (tab.value === 'tasks') tasks.value = await api.xianyuTasks();
  else {
    const body = {
      page_no: page.value,
      page_size: PAGE_SIZE,
      update_time: [
        Math.floor(Date.now() / 1000) - Number(days.value) * 86400,
        Math.floor(Date.now() / 1000),
      ],
    };
    if (status.value !== '')
      body[tab.value === 'products' ? 'product_status' : 'order_status'] = Number(status.value);
    if (tab.value === 'orders' && shopFilter.value) body.authorize_id = shopFilter.value;
    const data = await api.xianyuCall(tab.value, body);
    rows.value = listOf(data);
    count.value = Number(data.count || rows.value.length);
  }
  lastUpdated.value = new Date().toLocaleTimeString('zh-CN');
}
async function refresh() {
  await action(loadData);
}
async function saveConfig() {
  await action(async () => {
    const saved = await api.saveXianyuConfig(credentials);
    configured.value = saved.secretConfigured;
    credentials.appSecret = '';
    connected.value = false;
    shops.value = [];
    rows.value = [];
    tasks.value = [];
    detail.value = null;
    toast.success('凭据已保存在本机');
  });
}
async function saveHosting() {
  await action(async () => {
    const saved = await api.saveGiteeConfig(hosting);
    hostingConfigured.value = saved.tokenConfigured;
    hosting.token = '';
    hostingResult.value = '';
    toast.success('Gitee 配置已保存在本机');
  });
}
async function testHosting() {
  await action(async () => {
    const result = await api.testGitee();
    hostingResult.value =
      result.fullName + ' · 默认分支 ' + result.defaultBranch + ' · ' + result.message;
    toast.success('公开仓库可读取');
  });
}
async function testConnection() {
  await action(async () => {
    connected.value = false;
    await loadShops();
    toast.success('连接成功，已读取 ' + shops.value.length + ' 个店铺');
  });
}
async function viewProduct(id) {
  await action(async () => {
    const data = await api.xianyuCall('product', { product_id: id });
    detail.value = data;
    detailType.value = 'product';
    editShop.value = data.publish_shop?.[0]?.user_name || '';
    editor.price = Number(data.price || 0) / 100;
    editor.stock = data.stock ?? 0;
    syncEditor();
  });
}
function syncEditor() {
  const shop = productShops.value.find((s) => s.user_name === editShop.value);
  editor.title = shop?.title || detail.value?.title || '';
  editor.content = shop?.content || '';
}
watch(editShop, syncEditor);
async function viewOrder(orderNo) {
  await action(async () => {
    detail.value = await api.xianyuCall('order', { order_no: String(orderNo) });
    detailType.value = 'order';
    orderBookId.value = detail.value.localBook?.id || '';
    localQuery.value = '';
    localBooks.value = [];
    try {
      await findLocalBooks();
    } catch (e) {
      error.value = '本地图书加载失败：' + e.message;
    }
    Object.assign(shipping, {
      waybill_no: '',
      express_code: '',
      express_name: '',
      customSender: false,
      ship_name: '',
      ship_mobile: '',
      ship_district_id: '',
      ship_address: '',
    });
    if (!companies.value.length) {
      try {
        companies.value = listOf(await api.xianyuCall('companies'));
      } catch (e) {
        error.value = '快递公司加载失败：' + e.message;
      }
    }
  });
}
async function lookupDetail() {
  const id = lookup.value.trim();
  if (!id) return;
  if (tab.value === 'products') await viewProduct(id);
  else await viewOrder(id);
}
async function saveProduct() {
  await action(async () => {
    if (!editShop.value) throw new Error('请选择需要编辑的店铺');
    if (
      !editor.title.trim() ||
      weightedLength(editor.title) > 60 ||
      weightedLength(editor.content) < 5 ||
      weightedLength(editor.content) > 5000
    )
      throw new Error('请检查标题和描述长度');
    const payload = {
      product_id: detail.value.product_id,
      publish_shop: [{ user_name: editShop.value, title: editor.title, content: editor.content }],
    };
    if (singleSpec.value) payload.price = cents(editor.price);
    await api.xianyuCall('edit', payload);
    toast.success('编辑请求已受理；在架商品请稍后核查同步结果');
    detail.value = await api.xianyuCall('product', { product_id: detail.value.product_id });
    await loadData();
  });
}
async function saveStock() {
  await action(async () => {
    await api.xianyuCall('stock', {
      product_id: detail.value.product_id,
      stock: Number(editor.stock),
    });
    detail.value = await api.xianyuCall('product', { product_id: detail.value.product_id });
    toast.success('库存已更新');
    await loadData();
  });
}
async function shelf(op) {
  if (!window.confirm(op === 'publish' ? '确认将该商品上架到所选店铺？' : '确认下架该商品？'))
    return;
  await action(async () => {
    const body = { product_id: detail.value.product_id };
    if (op === 'publish') {
      if (!editShop.value) throw new Error('请选择上架店铺');
      body.user_name = [editShop.value];
    }
    await api.xianyuCall(op, body);
    toast.success('请求已受理，请刷新核查远端状态');
    detail.value = await api.xianyuCall('product', { product_id: detail.value.product_id });
    await loadData();
  });
}
watch(
  () => shipping.express_code,
  (code) => {
    shipping.express_name =
      code === 'other' ? '' : companies.value.find((c) => c.code === code)?.express_name || '';
  },
);
async function shipOrder() {
  if (!window.confirm('确认按填写的物流信息发货？')) return;
  await action(async () => {
    const body = {
      order_no: detail.value.order_no,
      waybill_no: shipping.waybill_no.trim(),
      express_code: shipping.express_code,
      express_name: shipping.express_name.trim(),
    };
    if (shipping.customSender)
      Object.assign(body, {
        ship_name: shipping.ship_name.trim(),
        ship_mobile: shipping.ship_mobile.trim(),
        ship_district_id: Number(shipping.ship_district_id),
        ship_address: shipping.ship_address.trim(),
      });
    await api.xianyuCall('ship', body);
    toast.success('发货请求已受理');
    detail.value = await api.xianyuCall('order', { order_no: detail.value.order_no });
    await loadData();
  });
}
async function taskAction(task, operation) {
  if (operation === 'publish' && !window.confirm('确认上架这条已创建的商品？')) return;
  await action(async () => {
    const updated = await api.xianyuTask(task.id, operation);
    tasks.value = tasks.value.map((t) => (t.id === updated.id ? updated : t));
    if (updated.error) toast.error(updated.error);
    else toast.success(operation === 'verify-images' ? '已记录图片正常显示' : '发布记录已更新');
    emit('updated');
  });
}
function changeTab(value) {
  if (busy.value || value === tab.value) return;
  tab.value = value;
  page.value = 1;
  status.value = '';
  keyword.value = '';
  lookup.value = '';
  rows.value = [];
  count.value = 0;
  error.value = '';
  refresh();
}
async function changePage(delta) {
  page.value += delta;
  await refresh();
}
async function filterChanged() {
  page.value = 1;
  rows.value = [];
  await refresh();
}
async function reloadTasks() {
  const cfg = await api.xianyuConfig();
  configured.value = cfg.secretConfigured;
  if (configured.value) {
    tasks.value = await api.xianyuTasks();
    tab.value = 'tasks';
  }
}
defineExpose({ refresh, reloadTasks, openOrder });
onMounted(async () => {
  await action(async () => {
    const [cfg, gitee] = await Promise.all([api.xianyuConfig(), api.giteeConfig()]);
    credentials.appKey = cfg.appKey;
    configured.value = cfg.secretConfigured;
    Object.assign(hosting, {
      repoUrl: gitee.repoUrl,
      branch: gitee.branch,
      directory: gitee.directory,
    });
    hostingConfigured.value = gitee.tokenConfigured;
    if (!configured.value) tab.value = 'config';
    else {
      await loadShops();
      tasks.value = await api.xianyuTasks();
    }
  });
  finishInit();
  timer = setInterval(async () => {
    if (busy.value || document.hidden || tab.value !== 'tasks' || !pendingTasks.value.length)
      return;
    await action(async () => {
      // Only monitor accepted publishes. Unknown creations require an explicit recovery lookup.
      for (const task of pendingTasks.value.filter((t) => t.productId).slice(0, 5)) {
        const updated = await api.xianyuTask(task.id, 'refresh');
        tasks.value = tasks.value.map((t) => (t.id === updated.id ? updated : t));
      }
      emit('updated');
    });
  }, 15000);
});
onBeforeUnmount(() => clearInterval(timer));
</script>

<template>
  <section class="xy-workbench">
    <header class="xy-heading">
      <div>
        <span class="xy-eyebrow">闲管家开放平台</span>
        <h2>闲鱼工作台</h2>
        <p>管理店铺、商品与订单，查看每一次接口发布的结果。</p>
      </div>
      <div class="head-actions">
        <span :class="['xy-connection', { connected }]">{{
          connected ? '已连接' : configured ? '已配置' : '待配置'
        }}</span
        ><UiButton class="btn accent" :disabled="!configured || busy" @click="emit('publish')">
          发布本地图书
        </UiButton>
      </div>
    </header>
    <nav class="xy-tabs" aria-label="闲鱼工作台功能">
      <UiButton
        v-for="(label, value) in tabs"
        :key="value"
        :class="{ active: tab === value }"
        :disabled="busy"
        @click="changeTab(value)"
      >
        {{ label }}<small v-if="value === 'tasks' && tasks.length">{{ tasks.length }}</small>
      </UiButton>
    </nav>
    <p v-if="error" class="xy-error" role="alert">
      {{ error }} <UiButton class="link-btn" :disabled="busy" @click="refresh">重试</UiButton>
    </p>
    <section v-if="tab === 'config'" class="panel xy-config">
      <h3>应用接入</h3>
      <p class="hint">使用已上线的自研进销存应用。凭据仅保存在本机，由 Rust 后端生成签名。</p>
      <form class="xy-config-form" @submit.prevent="saveConfig">
        <label class="field"
          ><span>AppKey</span
          ><input
            v-model="credentials.appKey"
            class="input mono"
            required
            autocomplete="off"
            :disabled="busy" /></label
        ><label class="field"
          ><span>AppSecret</span
          ><input
            v-model="credentials.appSecret"
            type="password"
            class="input"
            :placeholder="configured ? '已保存；留空保留原密钥' : '填写应用密钥'"
            autocomplete="new-password"
            :disabled="busy"
        /></label>
        <div class="head-actions">
          <UiButton class="btn" :disabled="busy">保存配置</UiButton
          ><UiButton
            type="button"
            class="btn ghost"
            :disabled="busy || !configured"
            @click="testConnection"
          >
            测试已保存的配置
          </UiButton>
        </div>
      </form>
      <hr class="xy-config-divider" />
      <h3>Gitee 图片托管</h3>
      <p class="hint">
        本地图片优先使用净化版，上传到你指定的公开仓库，再把匿名图片直链提交给闲管家。
      </p>
      <form class="xy-config-form" @submit.prevent="saveHosting">
        <label class="field"
          ><span>公开仓库链接</span
          ><input
            v-model="hosting.repoUrl"
            type="url"
            class="input"
            placeholder="https://gitee.com/owner/repository"
            required
            :disabled="busy"
        /></label>
        <label class="field"
          ><span>Gitee Token（需仓库写入权限）</span
          ><input
            v-model="hosting.token"
            type="password"
            class="input"
            :placeholder="hostingConfigured ? '已保存；留空保留原 Token' : '填写 Gitee 私人令牌'"
            autocomplete="new-password"
            :disabled="busy"
        /></label>
        <div class="form-grid two">
          <label class="field"
            ><span>分支（留空使用仓库默认分支）</span
            ><input v-model="hosting.branch" class="input" :disabled="busy" /></label
          ><label class="field"
            ><span>图片目录</span
            ><input v-model="hosting.directory" class="input" required :disabled="busy"
          /></label>
        </div>
        <div class="head-actions">
          <UiButton class="btn" :disabled="busy">保存图片托管配置</UiButton
          ><UiButton
            type="button"
            class="btn ghost"
            :disabled="busy || !hostingConfigured"
            @click="testHosting"
          >
            测试已保存的仓库
          </UiButton>
        </div>
      </form>
      <p v-if="hostingResult" class="xy-notice">{{ hostingResult }}</p>
      <p class="hint">
        上传前按图片内容生成文件名，相同图片复用已有文件。上传后的图片是公开文件；本地删除图片不会删除仓库文件。
      </p>
      <p class="hint">
        店铺授权及权益需在闲管家维护；本应用不提供账号授权。商品、订单查询受平台最近六个月和最多一万条的限制。
      </p>
    </section>
    <template v-else-if="configured">
      <div class="xy-toolbar">
        <template v-if="['products', 'orders'].includes(tab)"
          ><SearchSelect
            v-model="status"
            class="input select"
            aria-label="状态"
            :disabled="busy"
            @change="filterChanged"
          >
            <option value="">全部状态</option>
            <option
              v-for="(label, value) in tab === 'products' ? productStates : orderStates"
              :key="value"
              :value="value"
            >
              {{ label }}
            </option></SearchSelect
          ><SearchSelect
            v-if="tab === 'orders'"
            v-model="shopFilter"
            class="input select"
            aria-label="订单店铺"
            :disabled="busy"
            @change="filterChanged"
          >
            <option value="">全部店铺</option>
            <option v-for="shop in shops" :key="shop.authorize_id" :value="shop.authorize_id">
              {{ shop.shop_name || shop.user_nick }}
            </option></SearchSelect
          ><SearchSelect
            v-model="days"
            class="input select"
            aria-label="最近修改时间"
            :disabled="busy"
            @change="filterChanged"
          >
            <option :value="7">近 7 天修改</option>
            <option :value="30">近 30 天修改</option>
            <option :value="90">近 90 天修改</option>
            <option :value="180">近 180 天修改</option></SearchSelect
          ><input
            v-model="keyword"
            class="input xy-page-search"
            placeholder="筛选当前页"
            aria-label="筛选当前页"
          />
          <form class="xy-lookup" @submit.prevent="lookupDetail">
            <input
              v-model="lookup"
              class="input mono"
              :placeholder="tab === 'products' ? '闲管家商品 ID' : '闲鱼订单号'"
              :aria-label="tab === 'products' ? '闲管家商品 ID' : '闲鱼订单号'"
            /><UiButton class="btn ghost small" :disabled="busy">查询详情</UiButton>
          </form></template
        >
        <div class="spacer"></div>
        <span v-if="lastUpdated" class="hint">{{ lastUpdated }} 更新</span
        ><UiButton class="btn ghost small" :disabled="busy" @click="refresh">
          {{ busy ? '处理中…' : '刷新' }}
        </UiButton>
      </div>
      <template v-if="tab === 'shops'"
        ><div v-if="shops.length" class="xy-shop-grid">
          <article v-for="shop in shops" :key="shop.authorize_id" class="panel xy-shop">
            <div class="xy-shop-title">
              <div class="xy-shop-icon">鱼</div>
              <div>
                <h3>{{ shop.shop_name || shop.user_nick }}</h3>
                <span class="hint">{{ shop.user_name }}</span>
              </div>
              <span :class="['status', shop.is_valid ? 'listed' : 'failed']">{{
                shop.is_valid ? '权益有效' : '权益失效'
              }}</span>
            </div>
            <dl class="xy-facts">
              <div>
                <dt>店铺授权</dt>
                <dd>{{ date(shop.authorize_expires) }} 到期</dd>
              </div>
              <div>
                <dt>服务有效期</dt>
                <dd>{{ date(shop.valid_end_time) }}</dd>
              </div>
              <div>
                <dt>店铺类型</dt>
                <dd>
                  {{ shop.is_pro ? '专业卖家' : '普通卖家' }}{{ shop.is_trial ? ' · 试用中' : '' }}
                </dd>
              </div>
              <div>
                <dt>保证金</dt>
                <dd>{{ shop.is_deposit_enough ? '充足' : '不足' }}</dd>
              </div>
            </dl>
          </article>
        </div>
        <div v-else class="panel empty">
          <p>{{ busy ? '正在读取店铺…' : '尚无可用店铺' }}</p>
          <span>请检查应用配置及闲管家中的店铺授权。</span>
        </div></template
      >
      <section v-if="['products', 'orders'].includes(tab)" class="panel">
        <p v-if="tab === 'products'" class="xy-table-note">
          商品列表覆盖当前应用的店铺，标题与详情按需读取；“筛选当前页”仅过滤当前页数据。
        </p>
        <div class="table-wrap xy-table-wrap">
          <table class="book-table">
            <thead>
              <tr v-if="tab === 'products'">
                <th>商品 / 管家 ID</th>
                <th>状态</th>
                <th>售价</th>
                <th>库存</th>
                <th>商家编码</th>
                <th>更新时间</th>
                <th></th>
              </tr>
              <tr v-else>
                <th>订单 / 商品</th>
                <th>买家 / 店铺</th>
                <th>状态</th>
                <th>实付</th>
                <th>下单时间</th>
                <th>物流单号</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-if="!visibleRows.length">
                <td colspan="7" class="empty">
                  <p>{{ busy ? '正在查询…' : '没有符合条件的数据' }}</p>
                  <span>可调整状态、时间范围，或通过 ID 查询详情。</span>
                </td>
              </tr>
              <tr
                v-for="row in visibleRows"
                :key="tab === 'products' ? row.product_id : row.order_no"
                class="book-row"
                @click="tab === 'products' ? viewProduct(row.product_id) : viewOrder(row.order_no)"
              >
                <template v-if="tab === 'products'"
                  ><td>
                    <div class="book-title">{{ row.title || '—' }}</div>
                    <span class="hint mono">{{ row.product_id }}</span>
                  </td>
                  <td>
                    <span class="status">{{
                      productStates[row.product_status] || row.product_status
                    }}</span>
                  </td>
                  <td>{{ yuan(row.price) }}</td>
                  <td>{{ row.stock }}</td>
                  <td class="mono">{{ row.outer_id || '—' }}</td>
                  <td>{{ date(row.update_time) }}</td>
                  <td><UiButton class="btn tiny ghost" :disabled="busy">管理</UiButton></td></template
                ><template v-else
                  ><td>
                    <div class="book-title">{{ row.goods?.title || '—' }}</div>
                    <span class="hint mono">{{ row.order_no }}</span>
                    <div v-if="row.localBook">
                      <UiButton class="link-btn" @click.stop="openLocalBook(row.localBook)">
                        本地图书 #{{ row.localBook.id }} · {{ row.localBook.title }}
                      </UiButton>
                    </div>
                    <small v-else class="hint">未关联本地图书，可在详情中指定</small>
                  </td>
                  <td>
                    {{ row.buyer_nick || '—' }}<br /><span class="hint">{{ row.seller_name }}</span>
                  </td>
                  <td>
                    <span class="status">{{
                      orderStates[row.order_status] || row.order_status
                    }}</span
                    ><small v-if="row.refund_status" class="xy-refund"
                      >退款状态 {{ row.refund_status }}</small
                    >
                  </td>
                  <td>{{ yuan(row.pay_amount) }}</td>
                  <td>{{ date(row.order_time) }}</td>
                  <td class="mono">{{ row.waybill_no || '—' }}</td>
                  <td><UiButton class="btn tiny ghost" :disabled="busy">详情</UiButton></td></template
                >
              </tr>
            </tbody>
          </table>
        </div>
        <footer class="xy-pagination">
          <span>共 {{ count }} 条 · 第 {{ page }}/{{ totalPages }} 页</span>
          <div class="head-actions">
            <UiButton class="btn ghost small" :disabled="busy || page <= 1" @click="changePage(-1)">
              上一页</UiButton
            ><UiButton
              class="btn ghost small"
              :disabled="busy || page >= totalPages"
              @click="changePage(1)"
            >
              下一页
            </UiButton>
          </div>
        </footer>
      </section>
      <section v-if="tab === 'tasks'" class="xy-task-list">
        <p class="hint">
          图片方案须在上架成功、且闲鱼页面图片正常显示后才算验证通过。创建超时的任务请先核查，避免重复创建。
        </p>
        <article v-for="task in tasks" :key="task.id" class="panel xy-task">
          <div class="xy-task-title">
            <div>
              <h3>{{ task.title }}</h3>
              <span class="hint"
                >#{{ task.id }} · {{ task.userName }} · {{ imageModes[task.imageMode] }} ·
                {{ new Date(task.updatedAt).toLocaleString('zh-CN') }}</span
              >
            </div>
            <span
              :class="[
                'status',
                task.status === 'published' ? 'listed' : task.error ? 'failed' : 'filling',
              ]"
              >{{ taskStates[task.status] || task.status }}</span
            >
          </div>
          <p class="hint mono">
            管家商品 ID：{{ task.productId || '尚未取得' }} · 闲鱼 ID：{{
              task.itemId || '尚未取得'
            }}
          </p>
          <p v-if="task.error" class="xy-error">{{ task.error }}</p>
          <details v-if="task.attempts.length">
            <summary>查看尝试结果（{{ task.attempts.length }}）</summary>
            <p v-for="(attempt, index) in task.attempts" :key="index" class="hint">
              {{ imageModes[attempt.mode] }} · {{ attempt.result }}
            </p>
          </details>
          <div class="xy-task-actions">
            <UiButton class="btn ghost small" :disabled="busy" @click="taskAction(task, 'refresh')">
              刷新 / 核查</UiButton
            ><UiButton
              v-if="task.productId && ['created', 'offline', 'failed'].includes(task.status)"
              class="btn small"
              :disabled="busy"
              @click="taskAction(task, 'publish')"
            >
              继续上架</UiButton
            ><UiButton
              v-if="task.productId"
              class="btn ghost small"
              :disabled="busy"
              @click="viewProduct(task.productId)"
            >
              商品详情</UiButton
            ><a
              v-if="task.itemId"
              class="btn ghost small"
              :href="itemUrl(task.itemId)"
              target="_blank"
              rel="noopener noreferrer"
              >打开闲鱼商品 ↗</a
            ><UiButton
              v-if="task.status === 'published' && !task.imagesVerified"
              class="btn small"
              :disabled="busy"
              @click="taskAction(task, 'verify-images')"
            >
              已核对页面，图片正常</UiButton
            ><span v-if="task.imagesVerified" class="xy-verified">✓ 已人工核验图片</span>
          </div>
        </article>
        <div v-if="!tasks.length" class="panel empty">
          <p>还没有接口发布记录</p>
          <span>从书库或右上角选择本地图书开始发布。</span>
        </div>
      </section>
    </template>
    <div v-else class="panel empty">
      <p>请先配置应用凭据</p>
      <UiButton class="btn" @click="changeTab('config')">接入设置</UiButton>
    </div>

    <div v-if="detail" class="xy-overlay" @click.self="!busy && (detail = null)">
      <section class="xy-dialog" role="dialog" aria-modal="true" aria-labelledby="xy-detail-title">
        <header class="xy-dialog-head">
          <div>
            <span class="xy-eyebrow">{{ detailType === 'product' ? '商品管理' : '订单详情' }}</span>
            <h2 id="xy-detail-title">
              {{
                detailType === 'product'
                  ? detail.title || editor.title
                  : detail.goods?.title || detail.order_no
              }}
            </h2>
          </div>
          <UiButton class="icon-btn" :disabled="busy" aria-label="关闭" @click="detail = null">
            ×
          </UiButton>
        </header>
        <div class="xy-dialog-body">
          <p v-if="error" class="xy-error" role="alert">{{ error }}</p>
          <template v-if="detailType === 'product'"
            ><dl class="xy-facts">
              <div>
                <dt>管家商品 ID</dt>
                <dd class="mono">{{ detail.product_id }}</dd>
              </div>
              <div>
                <dt>商品状态</dt>
                <dd>
                  {{ productStates[detail.product_status] || detail.product_status }} · 发布状态
                  {{ detail.publish_status }}
                </dd>
              </div>
              <div>
                <dt>商家编码</dt>
                <dd>{{ detail.outer_id || '—' }}</dd>
              </div>
            </dl>
            <label class="field"
              ><span>操作店铺</span
              ><SearchSelect v-model="editShop" class="input" :disabled="busy">
                <option v-for="shop in productShops" :key="shop.user_name" :value="shop.user_name">
                  {{ shop.user_name
                  }}{{ shop.item_id && shop.item_id !== '0' ? ' · ' + shop.item_id : '' }}
                </option>
              </SearchSelect></label
            >
            <div class="xy-preview-images">
              <img
                v-for="(src, i) in productShops.find((s) => s.user_name === editShop)?.images || []"
                :key="i"
                :src="src"
                alt="远端商品图片"
              />
            </div>
            <p class="hint">查看返回图片仅供预览；发布记录的图片核验以闲鱼商品页面为准。</p>
            <form @submit.prevent="saveProduct">
              <fieldset :disabled="busy" class="xy-fieldset">
                <label class="field"
                  ><span>标题（中文计 2，最多 60 字符）</span
                  ><input v-model="editor.title" class="input" required /></label
                ><label class="field"
                  ><span>描述</span
                  ><textarea
                    v-model="editor.content"
                    class="input textarea"
                    rows="7"
                    required
                  /></label
                ><label v-if="singleSpec" class="field"
                  ><span>售价（元）</span
                  ><input
                    v-model="editor.price"
                    type="number"
                    min="0.01"
                    step="0.01"
                    class="input"
                    required
                /></label>
                <p v-else class="hint">多规格商品仅编辑文案；价格和库存请在闲管家管理。</p>
                <UiButton class="btn" :disabled="busy">保存商品修改</UiButton>
              </fieldset>
            </form>
            <form v-if="singleSpec" class="xy-inline-form" @submit.prevent="saveStock">
              <label class="field"
                ><span>库存（可设为 0）</span
                ><input
                  v-model="editor.stock"
                  type="number"
                  min="0"
                  max="9999"
                  class="input"
                  required
                  :disabled="busy" /></label
              ><UiButton class="btn ghost" :disabled="busy">更新库存</UiButton>
            </form>
            <div class="xy-task-actions">
              <UiButton
                class="btn"
                :disabled="busy || !editShop || detail.product_status === 22"
                @click="shelf('publish')"
              >
                上架到所选店铺</UiButton
              ><UiButton
                class="btn ghost"
                :disabled="busy || detail.product_status !== 22"
                @click="shelf('unpublish')"
              >
                下架商品
              </UiButton>
            </div></template
          >
          <template v-else
            ><dl class="xy-facts">
              <div>
                <dt>订单号</dt>
                <dd class="mono">{{ detail.order_no }}</dd>
              </div>
              <div>
                <dt>订单状态</dt>
                <dd>
                  {{ orderStates[detail.order_status] || detail.order_status }} · 退款状态
                  {{ detail.refund_status || 0 }}
                </dd>
              </div>
              <div>
                <dt>买家 / 店铺</dt>
                <dd>{{ detail.buyer_nick }} / {{ detail.seller_name }}</dd>
              </div>
              <div>
                <dt>实付 / 运费</dt>
                <dd>{{ yuan(detail.pay_amount) }} / {{ yuan(detail.express_fee) }}</dd>
              </div>
              <div>
                <dt>收件人</dt>
                <dd>{{ detail.receiver_name }} {{ detail.receiver_mobile }}</dd>
              </div>
              <div>
                <dt>收件地址</dt>
                <dd>
                  {{
                    [
                      detail.prov_name,
                      detail.city_name,
                      detail.area_name,
                      detail.town_name,
                      detail.address,
                    ]
                      .filter(Boolean)
                      .join(' ')
                  }}
                </dd>
              </div>
              <div>
                <dt>物流信息</dt>
                <dd>
                  {{ detail.express_name || detail.express_code || '未发货' }}
                  {{ detail.waybill_no }}
                </dd>
              </div>
              <div>
                <dt>卖家备注</dt>
                <dd>{{ detail.seller_remark || '—' }}</dd>
              </div>
            </dl>
            <section class="block">
              <h3>本地图书与库存</h3>
              <template v-if="detail.localBook">
                <UiButton class="link-btn" @click="openLocalBook(detail.localBook)">
                  #{{ detail.localBook.id }} {{ detail.localBook.title }} · 打开图书
                </UiButton>
                <p class="hint">
                  {{ detail.bookLinkSource === 'auto' ? '按发布记录自动关联' : '手动关联' }} ·
                  本地库存 {{ detail.localBook.stock }}
                </p>
              </template>
              <p v-else class="hint">尚未关联本地图书，请搜索并选择。不会根据相同书名自动关联。</p>
              <p>
                闲管家当前商品库存：<strong>{{
                  detail.remoteInventory?.stock ?? '查询失败'
                }}</strong>
                · 本单购买数量 {{ detail.goods?.quantity ?? '—' }}
              </p>
              <p>本单规格库存：{{ detail.remoteInventory?.skuStock ?? '未知' }}</p>
              <p v-if="detail.remoteInventory?.skuError" class="xy-error">
                {{ detail.remoteInventory.skuError }}
              </p>
              <p v-if="detail.remoteInventory?.error" class="xy-error" role="alert">
                {{ detail.remoteInventory.error }}
              </p>
              <p v-if="detail.remoteInventory?.checkedAt" class="hint">
                查询于
                {{
                  new Date(detail.remoteInventory.checkedAt).toLocaleString('zh-CN')
                }}，点击底部刷新详情可重新查询。关联不改变库存。
              </p>
              <div class="form-grid two">
                <label class="field"
                  ><span>查找本地图书</span
                  ><input
                    v-model="localQuery"
                    class="input"
                    placeholder="书名 / ISBN / 作者"
                    @keydown.enter.prevent="action(findLocalBooks)"
                /></label>
                <UiButton class="btn ghost" :disabled="busy" @click="action(findLocalBooks)">
                  搜索图书
                </UiButton>
                <label class="field span-2"
                  ><span>关联图书（搜索结果最多 100 本）</span
                  ><SearchSelect v-model="orderBookId" class="input" :disabled="busy">
                    <option value="">请选择</option>
                    <option
                      v-if="
                        detail.localBook && !localBooks.some((b) => b.id === detail.localBook.id)
                      "
                      :value="detail.localBook.id"
                    >
                      #{{ detail.localBook.id }} {{ detail.localBook.title }}
                    </option>
                    <option v-for="book in localBooks" :key="book.id" :value="book.id">
                      #{{ book.id }} {{ book.title }} · 库存 {{ book.stock }}
                    </option>
                  </SearchSelect></label
                >
              </div>
              <div class="head-actions">
                <UiButton class="btn" :disabled="busy || !orderBookId" @click="saveOrderBook()">
                  保存关联
                </UiButton>
                <UiButton
                  v-if="detail.localBook"
                  class="btn ghost"
                  :disabled="busy"
                  @click="saveOrderBook(true)"
                >
                  解除关联
                </UiButton>
              </div>
            </section>
            <form v-if="detail.order_status === 12" @submit.prevent="shipOrder">
              <h3>物流发货</h3>
              <fieldset :disabled="busy" class="xy-fieldset form-grid two">
                <label class="field"
                  ><span>快递公司</span
                  ><SearchSelect v-model="shipping.express_code" class="input" required>
                    <option value="">选择快递公司</option>
                    <option v-for="company in companies" :key="company.code" :value="company.code">
                      {{ company.express_name }}
                    </option>
                  </SearchSelect></label
                ><label class="field"
                  ><span>快递单号</span
                  ><input
                    v-model="shipping.waybill_no"
                    class="input mono"
                    required
                    minlength="10" /></label
                ><label v-if="shipping.express_code === 'other'" class="field span-2"
                  ><span>实际快递公司名称</span
                  ><input v-model="shipping.express_name" class="input" required /></label
                ><label class="xy-check span-2"
                  ><input
                    v-model="shipping.customSender"
                    type="checkbox"
                  />填写本次寄件人信息</label
                ><template v-if="shipping.customSender"
                  ><label class="field"
                    ><span>寄件人</span
                    ><input v-model="shipping.ship_name" class="input" required /></label
                  ><label class="field"
                    ><span>寄件手机号</span
                    ><input
                      v-model="shipping.ship_mobile"
                      class="input"
                      required
                      pattern="1[3-9][0-9]{9}" /></label
                  ><label class="field"
                    ><span>寄件地区编码</span
                    ><input
                      v-model="shipping.ship_district_id"
                      type="number"
                      min="1"
                      class="input"
                      required /></label
                  ><label class="field"
                    ><span>详细地址</span
                    ><input v-model="shipping.ship_address" class="input" required /></label
                ></template>
                <p v-else class="hint span-2">
                  使用闲管家后台配置的默认发货地址，请确认该地址已填写。
                </p>
                <UiButton class="btn accent" :disabled="busy">确认物流发货</UiButton>
              </fieldset>
            </form></template
          >
        </div>
        <footer class="xy-dialog-foot">
          <span class="hint">远端操作结果以闲管家 / 闲鱼返回为准。</span>
          <div class="spacer"></div>
          <UiButton
            class="btn ghost"
            :disabled="busy"
            @click="
              detailType === 'product' ? viewProduct(detail.product_id) : viewOrder(detail.order_no)
            "
          >
            刷新详情</UiButton
          ><UiButton class="btn" :disabled="busy" @click="detail = null">关闭</UiButton>
        </footer>
      </section>
    </div>
  </section>
</template>
