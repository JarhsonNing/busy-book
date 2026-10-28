<script setup>
import {
  computed,
  onBeforeUnmount,
  onMounted,
  reactive,
  ref,
  watch,
} from "vue";
import { api } from "../api";
import { useToast } from "../composables/toast";
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
} from "../xianyu";
import FulfillmentEditor from "./redesign/FulfillmentEditor.vue";
import OrderShipmentDialog from "./redesign/OrderShipmentDialog.vue";
import { fulfillmentStates } from "../commerce";
const shipmentOrder = ref("");
const emit = defineEmits(["publish", "updated", "open-book", "settings"]);
const toast = useToast();
const PAGE_SIZE = 100;
const tab = ref("orders"),
  busy = ref(false),
  error = ref(""),
  configured = ref(false);
const shops = ref([]),
  rows = ref([]),
  tasks = ref([]),
  count = ref(0),
  page = ref(1),
  status = ref("12"),
  shopFilter = ref(""),
  keyword = ref(""),
  lookup = ref("");
const days = ref(30),
  detail = ref(null),
  detailType = ref("");
const editShop = ref(""),
  editor = reactive({ title: "", content: "", price: "", stock: 0 });
const lastUpdated = ref("");
let timer;
let finishInit;
const initialized = new Promise((resolve) => {
  finishInit = resolve;
});
const localBooks = ref([]),
  localQuery = ref(""),
  orderBookId = ref("");
async function findLocalBooks() {
  localBooks.value = (
    await api.commerce(
      "/library?" + new URLSearchParams({ q: localQuery.value, pageSize: 100 }),
    )
  ).items;
}
async function saveOrderBook(clear = false) {
  await action(async () => {
    if (!clear && !orderBookId.value) throw new Error("请选择本地图书");
    detail.value = await api.linkXianyuOrder(
      detail.value.order_no,
      clear ? null : Number(orderBookId.value),
    );
    orderBookId.value = detail.value.localBook?.id || "";
    rows.value = rows.value.map((row) =>
      row.order_no === detail.value.order_no ? detail.value : row,
    );
    toast.success(clear ? "已解除本地关联" : "已关联本地图书");
    emit("updated");
  });
}
function openLocalBook(book) {
  detail.value = null;
  emit("open-book", book.id);
}
async function openOrder(orderNo) {
  await initialized;
  tab.value = "orders";
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
const totalPages = computed(() =>
  Math.max(1, Math.min(100, Math.ceil(count.value / PAGE_SIZE))),
);
const productShops = computed(() => detail.value?.publish_shop || []);
const singleSpec = computed(
  () =>
    !detail.value?.sku_items?.length &&
    Number(detail.value?.spec_type || 1) === 1,
);
const pendingTasks = computed(() =>
  tasks.value.filter((t) => ["publishing", "preparing"].includes(t.status)),
);
async function action(work) {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
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
  shops.value = listOf(await api.xianyuCall("shops"));
}
async function loadData() {
  if (!configured.value) return;
  if (tab.value === "tasks") tasks.value = await api.xianyuTasks();
  else if (tab.value === "orders" && status.value === "after_sale") {
    await api.commerce("/sync-orders", {});
    const orders = (await api.commerce("/fulfillments")).filter((f) =>
      ["after_sale", "canceled_review"].includes(f.state),
    );
    const shop = shops.value.find(
      (s) => String(s.authorize_id) === String(shopFilter.value),
    );
    const filtered = orders.filter(
      (f) => !shop || f.snapshot.seller_name === shop.user_name,
    );
    count.value = filtered.length;
    rows.value = filtered
      .slice((page.value - 1) * PAGE_SIZE, page.value * PAGE_SIZE)
      .map((f) => ({ ...f.snapshot, order_no: f.orderNo, fulfillment:f }));
  } else {
    const body = {
      page_no: page.value,
      page_size: PAGE_SIZE,
      update_time: [
        Math.floor(Date.now() / 1000) - Number(days.value) * 86400,
        Math.floor(Date.now() / 1000),
      ],
    };
    if (status.value !== "")
      body[tab.value === "products" ? "product_status" : "order_status"] =
        Number(status.value);
    if (tab.value === "orders" && shopFilter.value)
      body.authorize_id = shopFilter.value;
    const data = await api.xianyuCall(tab.value, body);
    rows.value = listOf(data);
    count.value = Number(data.count || rows.value.length);
  }
  lastUpdated.value = new Date().toLocaleTimeString("zh-CN");
}
async function refresh() {
  await action(loadData);
}
async function viewProduct(id) {
  await action(async () => {
    const data = await api.xianyuCall("product", { product_id: id });
    detail.value = data;
    detailType.value = "product";
    editShop.value = data.publish_shop?.[0]?.user_name || "";
    editor.price = Number(data.price || 0) / 100;
    editor.stock = data.stock ?? 0;
    syncEditor();
  });
}
function syncEditor() {
  const shop = productShops.value.find((s) => s.user_name === editShop.value);
  editor.title = shop?.title || detail.value?.title || "";
  editor.content = shop?.content || "";
}
watch(editShop, syncEditor);
async function viewOrder(orderNo) {
  await action(async () => {
    detail.value = await api.xianyuCall("order", { order_no: String(orderNo) });
    detailType.value = "order";
    orderBookId.value = detail.value.localBook?.id || "";
    localQuery.value = "";
    localBooks.value = [];
    try {
      await findLocalBooks();
    } catch (e) {
      error.value = "本地图书加载失败：" + e.message;
    }
  });
}
async function lookupDetail() {
  const id = lookup.value.trim();
  if (!id) return;
  if (tab.value === "products") await viewProduct(id);
  else await viewOrder(id);
}
async function saveProduct() {
  await action(async () => {
    if (!editShop.value) throw new Error("请选择需要编辑的店铺");
    if (
      !editor.title.trim() ||
      weightedLength(editor.title) > 60 ||
      weightedLength(editor.content) < 5 ||
      weightedLength(editor.content) > 5000
    )
      throw new Error("请检查标题和描述长度");
    const payload = {
      product_id: detail.value.product_id,
      publish_shop: [
        {
          user_name: editShop.value,
          title: editor.title,
          content: editor.content,
        },
      ],
    };
    if (singleSpec.value) payload.price = cents(editor.price);
    await api.xianyuCall("edit", payload);
    toast.success("编辑请求已受理；在架商品请稍后核查同步结果");
    detail.value = await api.xianyuCall("product", {
      product_id: detail.value.product_id,
    });
    await loadData();
  });
}
async function saveStock() {
  await action(async () => {
    await api.xianyuCall("stock", {
      product_id: detail.value.product_id,
      stock: Number(editor.stock),
    });
    detail.value = await api.xianyuCall("product", {
      product_id: detail.value.product_id,
    });
    toast.success("库存已更新");
    await loadData();
  });
}
async function shelf(op) {
  if (
    !window.confirm(
      op === "publish" ? "确认将该商品上架到所选店铺？" : "确认下架该商品？",
    )
  )
    return;
  await action(async () => {
    const body = { product_id: detail.value.product_id };
    if (op === "publish") {
      if (!editShop.value) throw new Error("请选择上架店铺");
      body.user_name = [editShop.value];
    }
    await api.xianyuCall(op, body);
    toast.success("请求已受理，请刷新核查远端状态");
    detail.value = await api.xianyuCall("product", {
      product_id: detail.value.product_id,
    });
    await loadData();
  });
}
async function taskAction(task, operation) {
  if (operation === "publish" && !window.confirm("确认上架这条已创建的商品？"))
    return;
  await action(async () => {
    const updated = await api.xianyuTask(task.id, operation);
    tasks.value = tasks.value.map((t) => (t.id === updated.id ? updated : t));
    if (updated.error) toast.error(updated.error);
    else
      toast.success(
        operation === "verify-images" ? "已记录图片正常显示" : "发布记录已更新",
      );
    emit("updated");
  });
}
function changeTab(value) {
  if (busy.value || value === tab.value) return;
  tab.value = value;
  page.value = 1;
  status.value = value === "orders" ? "12" : "";
  keyword.value = "";
  lookup.value = "";
  rows.value = [];
  count.value = 0;
  error.value = "";
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
    tab.value = "tasks";
  }
}
const overview = ref(null);
async function loadOverview(e) {
  if (e.target.open) {
    try {
      overview.value = await api.commerce("/overview");
    } catch (e) {
      error.value = e.message;
    }
  }
}
async function reconnect() {
  await action(async () => {
    const cfg = await api.xianyuConfig();
    configured.value = cfg.secretConfigured;
    if (configured.value) {
      await loadShops();
      await loadData();
    }
  });
}
defineExpose({ refresh, reloadTasks, openOrder, reconnect });
onMounted(async () => {
  await reconnect();
  finishInit();
  timer = setInterval(async () => {
    if (
      busy.value ||
      document.hidden ||
      tab.value !== "tasks" ||
      !pendingTasks.value.length
    )
      return;
    await action(async () => {
      // Only monitor accepted publishes. Unknown creations require an explicit recovery lookup.
      for (const task of pendingTasks.value
        .filter((t) => t.productId)
        .slice(0, 5)) {
        const updated = await api.xianyuTask(task.id, "refresh");
        tasks.value = tasks.value.map((t) =>
          t.id === updated.id ? updated : t,
        );
      }
      emit("updated");
    });
  }, 15000);
});
onBeforeUnmount(() => clearInterval(timer));
</script>

<template>
  <section class="commerce-module">
    <div class="module-bar">
      <nav class="page-tabs" aria-label="闲管家业务">
        <button
          :class="{ active: tab === 'orders' }"
          @click="changeTab('orders')"
        >
          订单</button
        ><button
          :class="{ active: tab !== 'orders' }"
          @click="changeTab('products')"
        >
          商品与库存
        </button>
      </nav>
      <select
        v-if="tab === 'orders'"
        v-model="shopFilter"
        class="input shop-select"
        aria-label="店铺"
        :disabled="tab !== 'orders'"
        @change="filterChanged"
      >
        <option value="">全部店铺</option>
        <option
          v-for="s in shops"
          :key="s.authorize_id"
          :value="s.authorize_id"
        >
          {{ s.shop_name || s.user_name || s.authorize_id }}
        </option></select
      ><span v-else class="muted" title="当前商品接口返回应用下全部店铺商品"
        >全部店铺</span
      >
    </div>
    <p v-if="error" class="error-panel" role="alert">
      {{ error }}
      <button class="link-btn" :disabled="busy" @click="refresh">重试</button>
    </p>
    <div v-if="!configured && !busy" class="empty-state">
      <h3>连接闲管家</h3>
      <button class="btn primary" @click="$emit('settings')">
        配置平台连接
      </button>
    </div>
    <template v-else>
      <div v-if="tab !== 'orders'" class="sub-tabs">
        <button
          :class="{ active: tab === 'products' }"
          @click="changeTab('products')"
        >
          在售商品</button
        ><button
          :class="{ active: tab === 'tasks' }"
          @click="changeTab('tasks')"
        >
          发布记录</button
        ><button class="btn primary push-right" @click="$emit('publish')">
          发布
        </button>
      </div>
      <template v-if="tab === 'orders' || tab === 'products'">
        <div class="filter-bar">
          <select
            v-model="status"
            class="input"
            aria-label="状态"
            @change="filterChanged"
          >
            <option value="">全部状态</option>
            <option v-if="tab === 'orders'" value="after_sale">
              售后待处理
            </option>
            <option
              v-for="(label, key) in tab === 'orders'
                ? orderStates
                : productStates"
              :key="key"
              :value="key"
            >
              {{ label }}
            </option></select
          ><select
            v-model="days"
            class="input"
            aria-label="时间范围"
            @change="filterChanged"
          >
            <option :value="7">近 7 天</option>
            <option :value="30">近 30 天</option>
            <option :value="90">近 90 天</option>
            <option :value="365">近一年</option></select
          ><input
            v-model="keyword"
            class="input search-input"
            :placeholder="tab === 'orders' ? '筛选本页订单' : '筛选本页商品'"
            aria-label="筛选当前页"
          />
          <form class="row push-right" @submit.prevent="lookupDetail">
            <input
              v-model="lookup"
              class="input"
              :placeholder="tab === 'orders' ? '输入订单号' : '输入商品 ID'"
              aria-label="精确查询编号"
            /><button class="btn" :disabled="busy || !lookup.trim()">
              查询
            </button>
          </form>
        </div>
        <div class="table-wrap" :aria-busy="busy">
          <table class="data-table">
            <thead>
              <tr v-if="tab === 'orders'">
                <th>商品 / 订单</th>
                <th>买家 / 店铺</th>
                <th>订单状态</th>
                <th class="numeric">实收</th>
                <th>下单时间</th>
                <th>操作</th>
              </tr>
              <tr v-else>
                <th>商品</th>
                <th>状态</th>
                <th class="numeric">售价</th>
                <th class="numeric">渠道 / 本地库存</th>
                <th>更新时间</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              <template
                v-for="r in visibleRows"
                :key="r.order_no || r.product_id"
                ><tr v-if="tab === 'orders'">
                  <td class="title-cell">
                    <button class="text-button" @click="viewOrder(r.order_no)">
                      {{ r.goods?.title || "订单商品" }}</button
                    ><small class="mono">{{ r.order_no }}</small
                    ><button
                      v-if="r.localBook"
                      class="link-btn"
                      @click="openLocalBook(r.localBook)"
                    >
                      {{ r.localBook.title }}
                    </button>
                  </td>
                  <td>
                    {{ r.buyer_nick || "—"
                    }}<small>{{ r.seller_name || "—" }}</small>
                  </td>
                  <td>
                    <span
                      class="tag"
                      :class="
                        Number(r.order_status) === 12 ? 'warning' : 'neutral'
                      "
                      >{{ orderStates[r.order_status] || r.order_status }}</span
                    ><small v-if="r.refund_status"
                      >售后：{{ r.refund_status }}</small
                    ><small v-if="r.fulfillment?.state && !['shipped','completed','unmatched'].includes(r.fulfillment.state)">{{ fulfillmentStates[r.fulfillment.state] || r.fulfillment.state }}</small>
                  </td>
                  <td class="numeric">{{ yuan(r.pay_amount) }}</td>
                  <td>{{ date(r.order_time) }}</td>
                  <td class="order-actions">
                    <button v-if="r.fulfillment?.kind!=='digital' && Number(r.order_status)===12" class="btn small" @click="shipmentOrder=String(r.order_no)">发货</button>
                    <button v-else-if="r.fulfillment?.purchaseState" class="link-btn" @click="shipmentOrder=String(r.order_no)">采购与物流</button>
                    <button class="link-btn" @click="viewOrder(r.order_no)">详情</button>
                  </td>
                </tr>
                <tr v-else>
                  <td class="title-cell">
                    <button
                      class="text-button"
                      @click="viewProduct(r.product_id)"
                    >
                      {{ r.title || r.product_id }}</button
                    ><small class="mono">{{ r.product_id }}</small>
                  </td>
                  <td>
                    <span class="tag neutral">{{
                      productStates[r.product_status] || r.product_status
                    }}</span>
                  </td>
                  <td class="numeric">{{ yuan(r.price) }}</td>
                  <td class="numeric">
                    {{ r.stock ?? "—"
                    }}<small>本地 {{ r.localBook?.stock ?? "未关联" }}</small>
                  </td>
                  <td>{{ date(r.update_time) }}</td>
                  <td>
                    <button class="link-btn" @click="viewProduct(r.product_id)">
                      管理
                    </button>
                  </td>
                </tr></template
              >
            </tbody>
          </table>
          <div v-if="!visibleRows.length" class="empty-state">
            {{ busy ? "正在读取…" : "暂无记录" }}
          </div>
        </div>
        <footer class="pagination">
          <span class="muted"
            >{{ count }} 条 · 每页 {{ PAGE_SIZE }} 条
            <span v-if="lastUpdated">· {{ lastUpdated }} 更新</span></span
          >
          <div class="row">
            <button
              class="btn"
              :disabled="busy || page <= 1"
              @click="changePage(-1)"
            >
              上一页</button
            ><span>{{ page }} / {{ totalPages }}</span
            ><button
              class="btn"
              :disabled="busy || page >= totalPages"
              @click="changePage(1)"
            >
              下一页
            </button>
          </div>
        </footer>
        <details
          v-if="tab === 'orders'"
          class="detail-section"
          @toggle="loadOverview"
        >
          <summary>经营统计</summary>
          <div v-if="overview" class="metrics-inline">
            <section v-for="(m, key) in overview.metrics" :key="key">
              <h4>{{ key === "physical" ? "实体书" : "电子资源" }}</h4>
              <dl class="facts-grid">
                <dt>收入</dt>
                <dd>{{ yuan(m.revenue) }}</dd>
                <dt>成本</dt>
                <dd>{{ yuan(m.cost) }}</dd>
                <dt>利润</dt>
                <dd>{{ yuan(m.margin) }}</dd>
                <dt>待核算</dt>
                <dd>{{ m.pending ?? m.pendingCost ?? "—" }}</dd>
              </dl>
            </section>
          </div>
        </details>
      </template>
      <section v-if="tab === 'tasks'" class="task-list">
        <article v-for="t in tasks" :key="t.id" class="publish-record">
          <div class="module-bar">
            <div>
              <h3>{{ t.title }}</h3>
              <small
                >{{ t.userName }} · {{ imageModes[t.imageMode] }} ·
                {{ new Date(t.updatedAt).toLocaleString("zh-CN") }}</small
              >
            </div>
            <span class="tag" :class="t.error ? 'warning' : 'neutral'">{{
              taskStates[t.status] || t.status
            }}</span>
          </div>
          <p v-if="t.error" class="error-panel">{{ t.error }}</p>
          <div class="row">
            <button
              class="btn"
              :disabled="busy"
              @click="taskAction(t, 'refresh')"
            >
              核查结果</button
            ><button
              v-if="
                t.productId &&
                ['created', 'offline', 'failed'].includes(t.status)
              "
              class="btn"
              :disabled="busy"
              @click="taskAction(t, 'publish')"
            >
              继续上架</button
            ><button
              v-if="t.productId"
              class="btn"
              @click="viewProduct(t.productId)"
            >
              商品详情</button
            ><a
              v-if="t.itemId"
              class="link-btn"
              :href="itemUrl(t.itemId)"
              target="_blank"
              rel="noreferrer"
              >查看闲鱼商品 ↗</a
            ><button
              v-if="t.status === 'published' && !t.imagesVerified"
              class="btn"
              :disabled="busy"
              @click="taskAction(t, 'verify-images')"
            >
              确认图片显示正常</button
            ><span v-if="t.imagesVerified" class="tag success">图片已核验</span>
          </div>
          <details v-if="t.attempts?.length">
            <summary>执行记录</summary>
            <p v-for="(a, i) in t.attempts" :key="i">
              {{ imageModes[a.mode] }} · {{ a.result }}
            </p>
          </details>
        </article>
        <div v-if="!tasks.length" class="empty-state">暂无发布记录</div>
      </section>
    </template>
    <OrderShipmentDialog v-if="shipmentOrder" :order-no="shipmentOrder" @close="shipmentOrder=''" @updated="refresh" />
    <div
      v-if="detail"
      class="modal-backdrop"
      @click.self="!busy && (detail = null)"
    >
      <section
        class="modal-dialog wide-dialog"
        v-modal-focus="
          () => {
            !busy && (detail = null);
          }
        "
        role="dialog"
        aria-modal="true"
        aria-labelledby="commerce-detail-title"
        @keydown.esc="!busy && (detail = null)"
      >
        <header class="modal-header">
          <div>
            <small>{{
              detailType === "product" ? "商品与库存" : detail.order_no
            }}</small>
            <h2 id="commerce-detail-title">
              {{ detail.title || detail.goods?.title || "订单详情" }}
            </h2>
          </div>
          <button
            class="icon-btn"
            :disabled="busy"
            aria-label="关闭详情"
            @click="detail = null"
          >
            ×
          </button>
        </header>
        <div class="modal-body">
          <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
          <template v-if="detailType === 'product'"
            ><dl class="facts-grid">
              <dt>商品 ID</dt>
              <dd>{{ detail.product_id }}</dd>
              <dt>状态</dt>
              <dd>{{ productStates[detail.product_status] }}</dd>
              <dt>渠道可售数量</dt>
              <dd>{{ detail.stock ?? "—" }}</dd>
              <dt>本地库存</dt>
              <dd>{{ detail.localBook?.stock ?? "未关联" }}</dd>
              <dt>规格</dt>
              <dd>{{ singleSpec ? "单规格" : "多规格" }}</dd>
            </dl>
            <form @submit.prevent="saveProduct">
              <div class="form-grid">
                <label class="field wide"
                  ><span>店铺</span
                  ><select v-model="editShop" class="input">
                    <option
                      v-for="s in productShops"
                      :key="s.user_name"
                      :value="s.user_name"
                    >
                      {{ s.user_name }}
                    </option>
                  </select></label
                ><label class="field wide"
                  ><span>标题</span
                  ><input
                    v-model="editor.title"
                    class="input"
                    required /></label
                ><label class="field wide"
                  ><span>描述</span
                  ><textarea
                    v-model="editor.content"
                    class="input"
                    rows="4"
                  /></label
                ><label class="field"
                  ><span>售价</span
                  ><input
                    v-model="editor.price"
                    class="input"
                    type="number"
                    min="0.01"
                    step="0.01"
                    :disabled="!singleSpec"
                /></label>
              </div>
              <button class="btn primary" :disabled="busy">保存商品</button>
            </form>
            <section class="detail-section">
              <h3>渠道库存</h3>
              <div class="row">
                <input
                  v-model.number="editor.stock"
                  class="input"
                  type="number"
                  min="0"
                  step="1"
                  aria-label="渠道可售数量"
                  :disabled="!singleSpec"
                /><button
                  class="btn"
                  :disabled="busy || !singleSpec"
                  @click="saveStock"
                >
                  调整数量
                </button>
              </div>
              <p v-if="!singleSpec" class="muted">多规格库存请在闲管家管理。</p>
            </section>
            <div class="row">
              <button class="btn" :disabled="busy" @click="shelf('publish')">
                上架</button
              ><button
                class="btn danger"
                :disabled="busy"
                @click="shelf('unpublish')"
              >
                下架
              </button>
            </div></template
          >
          <template v-else
            ><dl class="facts-grid">
              <dt>状态</dt>
              <dd>{{ orderStates[detail.order_status] }}</dd>
              <dt>实收</dt>
              <dd>{{ yuan(detail.pay_amount) }}</dd>
              <dt>收件人</dt>
              <dd>{{ detail.receiver_name }} {{ detail.receiver_mobile }}</dd>
              <dt>地址</dt>
              <dd>
                {{ detail.prov_name }}{{ detail.city_name }}{{ detail.area_name
                }}{{ detail.town_name }}{{ detail.address }}
              </dd>
              <dt>物流</dt>
              <dd>
                {{ detail.express_name }} {{ detail.waybill_no || "暂无" }}
              </dd>
            </dl>
            <section class="detail-section">
              <div class="module-bar">
                <h3>关联图书</h3>
                <button
                  v-if="detail.localBook"
                  class="link-btn"
                  @click="openLocalBook(detail.localBook)"
                >
                  {{ detail.localBook.title }} ↗
                </button>
              </div>
              <div class="row">
                <input
                  v-model="localQuery"
                  class="input"
                  placeholder="书名 / ISBN"
                  aria-label="查找关联图书"
                  @keyup.enter="findLocalBooks"
                /><button class="btn" @click="findLocalBooks">查找</button
                ><select
                  v-model="orderBookId"
                  class="input"
                  aria-label="选择图书"
                >
                  <option value="">选择图书</option>
                  <option v-for="b in localBooks" :key="b.id" :value="b.id">
                    {{ b.title }} · {{ b.edition || b.isbn }}
                  </option></select
                ><button
                  class="btn"
                  :disabled="busy || !orderBookId"
                  @click="saveOrderBook()"
                >
                  关联
                </button>
              </div>
            </section>
            <FulfillmentEditor
              :key="detail.order_no"
              :order-no="detail.order_no"
              :book-id="detail.localBook?.id"
              @updated="viewOrder(detail.order_no)"
              @open-book="openLocalBook({ id: $event })"
          /></template>
        </div>
      </section>
    </div>
  </section>
</template>
