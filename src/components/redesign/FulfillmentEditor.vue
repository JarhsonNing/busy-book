<script setup>
import { reactive, ref, onMounted } from "vue";
import { api } from "../../api";
import { fulfillmentStates } from "../../commerce";
import OrderShipmentDialog from "./OrderShipmentDialog.vue";
const props = defineProps({ orderNo: String, bookId: Number });
const emit = defineEmits(["updated", "open-book"]);
const detail = ref(null),
  busy = ref(false),
  error = ref(""),
  message = ref(""),
  note = ref(""),
  closeCase = ref(false);
const shipmentOpen = ref(false);
const finance = reactive({ cost: null, fees: null, refund: null });
async function load() {
  error.value = "";
  try {
    const all = await api.commerce("/fulfillments?"+new URLSearchParams({orderNo:props.orderNo}));
    const found = all.find((f) => f.orderNo === props.orderNo);
    if (!found) {
      detail.value = null;
      return;
    }
    detail.value = await api.commerce("/fulfillments/" + found.id);
    Object.assign(
      finance,
      { cost: null, fees: null, refund: null },
      detail.value.data.finance || {},
    );
  } catch (e) {
    error.value = e.message;
  }
}
async function act(fn) {
  busy.value = true;
  error.value = "";
  message.value = "";
  try {
    await fn();
    await load();
    message.value = "已保存";
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
async function saveFinance() {
  await act(() =>
    api.commerce(
      "/fulfillments/" + detail.value.id + "/finance",
      Object.fromEntries(
        Object.entries(finance).map(([k, v]) => [k, v === "" ? null : v]),
      ),
    ),
  );
}
async function saveNote() {
  await act(async () => {
    await api.commerce("/fulfillments/" + detail.value.id + "/note", {
      note: note.value,
      close: closeCase.value,
    });
    note.value = "";
    closeCase.value = false;
  });
}
async function shipmentUpdated() { await load(); emit("updated"); }
async function refreshRemote() {
  await act(async () => {
    await api.commerce("/fulfillments/" + detail.value.id + "/refresh", {});
    emit("updated");
  });
}
onMounted(load);
</script>
<template>
  <section class="fulfillment-editor">
    <p v-if="error" class="error-panel" role="alert">
      {{ error }} <button class="link-btn" @click="load">重试</button>
    </p>
    <p v-if="message" class="success-text" role="status">{{ message }}</p>
    <template v-if="detail"
      ><div class="module-bar">
        <h3>采购与履约</h3>
        <span class="tag warning">{{
          fulfillmentStates[detail.state] || detail.state
        }}</span
        ><button class="link-btn" :disabled="busy" @click="refreshRemote">
          核查远端
        </button>
      </div>
      <p v-if="detail.kind === 'unknown'" class="notice">
        {{ detail.data.reason || "需要确认商品与规格映射"
        }}<button
          v-if="detail.bookId || bookId"
          class="link-btn"
          @click="$emit('open-book', detail.bookId || bookId)"
        >
          打开图书配置
        </button>
      </p>
      <div v-if="detail.kind !== 'digital'" class="shipment-summary">
        <div v-for="p in detail.purchases.filter(p=>!['canceled','replaced'].includes(p.state))" :key="p.id" class="list-row">
          <span>淘宝 {{ p.data.purchaseNo || '待关联' }}</span><span>{{ {draft:'待采购',ordered:'已下单待物流',shipped:'供应商已发货'}[p.state] || p.state }}</span><span>{{ p.data.expressName }} {{ p.data.trackingNo }}</span>
        </div>
        <p v-if="detail.data.shipping?.trackingNo" class="muted">内部物流：{{ detail.data.shipping.expressName }} {{ detail.data.shipping.trackingNo }}</p>
        <button type="button" class="btn primary" @click="shipmentOpen=true">{{ detail.state==='to_purchase' || detail.state==='unmatched' ? '发货' : '采购与物流' }}</button>
        <OrderShipmentDialog v-if="shipmentOpen" :order-no="orderNo" @close="shipmentOpen=false" @updated="shipmentUpdated" />
      </div>
      <div v-if="detail.kind === 'digital'">
        <div v-for="d in detail.deliveries" :key="d.id" class="list-row">
          <span>{{ d.state }}</span
          ><span>{{ d.updatedAt }}</span>
        </div>
        <p v-if="!detail.deliveries?.length" class="muted">暂无交付结果</p>
      </div>
      <details class="detail-section">
        <summary>核算</summary>
        <form @submit.prevent="saveFinance">
          <div class="form-grid">
            <label
              v-for="(label, key) in {
                cost: '成本',
                fees: '费用',
                refund: '退款',
              }"
              :key="key"
              class="field"
              ><span>{{ label }}</span
              ><input
                v-model.number="finance[key]"
                class="input"
                type="number"
                min="0"
                step="0.01"
                placeholder="待核算"
            /></label>
          </div>
          <button class="btn" :disabled="busy">保存核算</button>
        </form>
      </details>
      <details class="detail-section">
        <summary>售后与处理记录</summary>
        <form @submit.prevent="saveNote">
          <textarea
            v-model="note"
            class="input"
            rows="3"
            aria-label="处理记录"
            required
          /><label class="check-line"
            ><input type="checkbox" v-model="closeCase" />结案</label
          ><button class="btn" :disabled="busy">保存记录</button>
        </form>
        <div v-for="(e, i) in detail.events" :key="i" class="list-row">
          <span>{{ e.data?.note || e.type || e.event }}</span
          ><small>{{ e.createdAt }}</small>
        </div>
      </details>
      <details v-if="detail.purchases?.length" class="detail-section">
        <summary>采购历史 · {{ detail.purchases.length }}</summary>
        <div v-for="p in detail.purchases" :key="p.id" class="list-row">
          <span>{{ p.data.purchaseNo || "未下单" }} · {{ {draft:'待采购',ordered:'已下单待物流',shipped:'供应商已发货',canceled:'已取消',replaced:'已更换'}[p.state] || p.state }}</span
          ><span>{{ p.data.trackingNo }}</span
          ><span>¥{{ p.data.actualCost ?? "—" }}</span>
        </div>
      </details>
    </template>
    <p v-else-if="!error" class="muted">正在读取履约记录…</p>
  </section>
</template>
