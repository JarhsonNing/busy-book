<script setup>
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { api, openBrowser } from '../../api';
import { fulfillmentStates } from '../../commerce';
import { listOf } from '../../xianyu';
const props = defineProps({ orderNo: {type:String,required:true} });
const emit = defineEmits(['close','updated']);
const detail = ref(null), loading = ref(true), busy = ref(false), error = ref(''), message = ref('');
const mode = ref('manual'), confirmed = ref(false), companies = ref([]), companyError = ref('');
const formElement = ref(null);
const receiver = ref(null), receiverBusy = ref(false), receiverError = ref('');
const purchase = reactive({}), tracking = reactive({trackingNo:'',expressCode:'',expressName:''});
const current = computed(() => detail.value?.purchases?.find(p => !['canceled','replaced'].includes(p.state)));
const uncertain = computed(() => detail.value?.state==='shipping_unknown' || detail.value?.data.shippingUncertain);
const canShip = computed(() => detail.value?.snapshot.order_status===12 && !uncertain.value && detail.value.kind!=='digital' && [0,4,6].includes(Number(detail.value.snapshot.refund_status || 0)));
const logisticsVisible = computed(() => mode.value==='manual' || purchase.state==='shipped');
const confirmationLabel = computed(() => purchase.state==='canceled' && mode.value==='taobao' ? '已核实采购取消与退款' : logisticsVisible.value ? '已核对实物商品、数量、收件信息与物流' : '已核对实物商品、数量、收件信息与采购订单');
const activeLogistics = computed(() => mode.value==='manual' ? tracking : purchase);
const buyUrl = computed(() => purchase.url || current.value?.data.source?.data.url || detail.value?.data.source?.data.url || 'https://www.taobao.com/');
function close() { if (!busy.value) emit('close'); }
async function loadReceiver(event) {
  if (!event.target.open || receiver.value || receiverBusy.value) return;
  receiverBusy.value=true;receiverError.value='';
  try { receiver.value=await api.xianyuCall('order',{order_no:props.orderNo}); }
  catch(e) { receiverError.value=e.message; } finally { receiverBusy.value=false; }
}
async function load(reset=true) {
  const records = await api.commerce('/fulfillments?'+new URLSearchParams({orderNo:props.orderNo}));
  if (!records.length) throw new Error('未找到订单，请刷新闲鱼订单后重试');
  detail.value = await api.commerce('/fulfillments/'+records[0].id);
  if (!reset) return;
  const saved = current.value;
  mode.value = saved ? 'taobao' : 'manual';
  Object.assign(purchase,{sourceId:null,state:'ordered',purchaseNo:'',actualCost:null,supplierRefund:0,url:'',shop:'',sku:'',note:'',trackingNo:'',expressCode:'',expressName:''},saved?.data || {});
  if (saved) {
    purchase.state=saved.state; purchase.sourceId=saved.data.source?.id ?? null;
    purchase.url=saved.data.url || saved.data.source?.data.url || '';
    purchase.shop=saved.data.shop || saved.data.source?.data.shop || '';
    purchase.sku=saved.data.sku || saved.data.source?.data.sku || '';
  }
  Object.assign(tracking,{trackingNo:'',expressCode:'',expressName:''},detail.value.data.shipping || {});
  confirmed.value=false;
}
async function loadCompanies() {
  try {
    companies.value=listOf(await api.xianyuCall('companies')).map(c => ({code:String(c.express_code || c.code || ''),name:c.express_name || c.name || ''})).filter(c=>c.code && c.name);
    if (!companies.value.length) companyError.value='公司列表暂无数据，可手动填写';
  } catch { companyError.value='公司列表加载失败，可手动填写'; }
}
function selectCompany(event) {
  const company=companies.value.find(c=>c.code===event.target.value);
  activeLogistics.value.expressCode=company?.code || '';activeLogistics.value.expressName=company?.name || '';
}
async function buy() {
  error.value='';
  try { await openBrowser('taobao',buyUrl.value); } catch(e) { error.value=e.message; }
}
async function save(send=false) {
  if (busy.value) return;
  if (!formElement.value?.reportValidity()) return;
  error.value='';message.value='';
  if (!confirmed.value) { error.value='请先核对实物商品、数量、收件信息和物流';return; }
  if (send && !canShip.value) { error.value='当前订单不可发货，请先核查远端状态';return; }
  busy.value=true;
  const endpoint='/fulfillments/'+detail.value.id;
  try {
    if (mode.value==='manual') {
      await api.commerce(endpoint+'/tracking',{...tracking,confirmed:true,physicalConfirmed:true});
    } else {
      await api.commerce(endpoint+'/purchase',{...purchase,platform:'taobao',actualCost:purchase.actualCost===''?null:purchase.actualCost,confirmed:true,physicalConfirmed:true});
    }
    message.value=mode.value==='manual'?'物流已保存在内部':'采购关联已保存，仅内部可见';
    confirmed.value=false;
    await load(false);emit('updated');
    if (send) {
      const result=await api.commerce(endpoint+'/ship',{confirmed:true});
      message.value=result.status==='already_shipped'?'闲鱼订单已发货':'发货已受理，正在核查';
      try { await api.commerce(endpoint+'/refresh',{}); } catch { message.value='发货已受理，请点击核查状态确认结果'; }
      await load(false);emit('updated');
      if (detail.value.state==='shipped' || detail.value.state==='completed') message.value='已确认闲鱼发货成功';
    }
  } catch(e) {
    error.value=e.message;
    try { await load(false);emit('updated'); } catch { /* Preserve the original action error and current form. */ }
  } finally { busy.value=false; }
}
async function refresh() {
  if (busy.value) return;
  busy.value=true;error.value='';
  try { await api.commerce('/fulfillments/'+detail.value.id+'/refresh',{});await load(false);emit('updated'); }
  catch(e) { error.value=e.message; } finally { busy.value=false; }
}
watch(() => [mode.value,JSON.stringify(purchase),JSON.stringify(tracking)],()=>{confirmed.value=false;});
onMounted(async()=>{
  try { await load(); } catch(e) { error.value=e.message; }
  finally { loading.value=false; }
  if (detail.value) await loadCompanies();
});
</script>

<template>
  <Teleport to="body">
    <div class="modal-backdrop shipment-backdrop" @click.self="close">
      <section class="modal-dialog shipment-dialog" role="dialog" aria-modal="true" aria-labelledby="shipment-title" v-modal-focus="close">
        <header class="source-editor-header">
          <div><h2 id="shipment-title">发货</h2><p class="muted">闲鱼订单 {{ orderNo }}</p></div>
          <button class="icon-btn" type="button" aria-label="关闭发货" :disabled="busy" @click="close">×</button>
        </header>
        <div v-if="loading" class="source-editor-body" role="status">正在读取订单…</div>
        <div v-else-if="!detail" class="source-editor-body"><p class="error-panel" role="alert">{{ error }}</p><button class="btn" @click="loading=true;load().catch(e=>error=e.message).finally(()=>loading=false)">重试</button></div>
        <form v-else ref="formElement" class="source-editor-form" @submit.prevent="save(false)">
          <div class="source-editor-body shipment-body">
            <div class="shipment-status"><strong>{{ detail.snapshot.goods?.title || '订单商品' }} × {{ detail.snapshot.goods?.quantity }}</strong><span class="tag" :class="uncertain?'warning':'neutral'">{{ fulfillmentStates[detail.state] || detail.state }}</span><button type="button" class="link-btn" :disabled="busy" @click="refresh">核查状态</button></div>
            <details class="shipment-recipient" @toggle="loadReceiver"><summary>收件信息</summary><p v-if="receiverBusy" class="muted">正在读取…</p><template v-else-if="receiver"><p>{{ receiver.receiver_name }} {{ receiver.receiver_mobile }}</p><p>{{ receiver.prov_name }}{{ receiver.city_name }}{{ receiver.area_name }}{{ receiver.town_name }}{{ receiver.address }}</p></template><p v-if="receiverError" class="error-panel" role="alert">{{ receiverError }} <button type="button" class="link-btn" @click="loadReceiver({target:{open:true}})">重试</button></p></details>
            <p v-if="detail.kind==='digital'" class="notice">电子订单使用交付流程，无需填写快递。</p>
            <template v-else>
              <nav class="page-tabs shipment-tabs" aria-label="发货方式">
                <button type="button" :aria-pressed="mode==='manual'" :class="{active:mode==='manual'}" :disabled="busy" @click="mode='manual'">填写物流</button>
                <button type="button" :aria-pressed="mode==='taobao'" :class="{active:mode==='taobao'}" :disabled="busy" @click="mode='taobao'">淘宝采购</button>
              </nav>
              <fieldset :disabled="busy || uncertain || (mode==='manual' && !canShip)" class="shipment-fields">
                <div v-if="mode==='taobao'" class="shipment-grid">
                  <label class="field"><span>淘宝订单号</span><input v-model.trim="purchase.purchaseNo" class="input" :required="purchase.state!=='canceled'" :readonly="!!current && current.state!=='draft'" autocomplete="off" /></label>
                  <label class="field"><span>实付金额</span><input v-model.number="purchase.actualCost" class="input" type="number" min="0" step="0.01" placeholder="待核对" /></label>
                  <label class="field"><span>采购状态</span><select v-model="purchase.state" class="input"><option value="ordered" :disabled="current?.state==='shipped'">已下单待物流</option><option value="shipped">供应商已发货</option><option v-if="current" value="canceled">已取消 / 退款</option></select></label>
                  <label v-if="purchase.state==='canceled'" class="field"><span>退款金额</span><input v-model.number="purchase.supplierRefund" class="input" type="number" min="0" step="0.01" required /></label>
                  <label v-if="purchase.state==='canceled'" class="field shipment-span-2"><span>取消说明</span><input v-model.trim="purchase.note" class="input" required /></label>
                </div>
                <div v-if="logisticsVisible" class="shipment-grid shipment-logistics">
                  <label v-if="companies.length" class="field"><span>快递公司</span><select :value="activeLogistics.expressCode" class="input" required @change="selectCompany"><option value="">选择快递公司</option><option v-if="activeLogistics.expressCode && !companies.some(c=>c.code===activeLogistics.expressCode)" :value="activeLogistics.expressCode">{{ activeLogistics.expressName }}</option><option v-for="c in companies" :key="c.code" :value="c.code">{{ c.name }}</option></select></label>
                  <template v-else><label class="field"><span>快递公司</span><input v-model.trim="activeLogistics.expressName" class="input" required /></label><label class="field"><span>快递编码</span><input v-model.trim="activeLogistics.expressCode" class="input" required /></label></template>
                  <label class="field" :class="{'shipment-span-2':companies.length}"><span>快递单号</span><input v-model.trim="activeLogistics.trackingNo" class="input" required autocomplete="off" /></label>
                </div>
                <p v-if="logisticsVisible && companyError" class="muted">{{ companyError }}</p>
                <div v-if="mode==='taobao'" class="shipment-additional">
                  <details><summary>商品与内部备注</summary><div class="shipment-grid">
                    <label class="field"><span>店铺</span><input v-model.trim="purchase.shop" class="input" /></label>
                    <label class="field shipment-span-2"><span>版本 / 规格</span><input v-model.trim="purchase.sku" class="input" /></label>
                    <label class="field shipment-span-3"><span>商品链接</span><input v-model.trim="purchase.url" class="input" type="url" placeholder="选填" /></label>
                    <label v-if="purchase.state!=='canceled'" class="field shipment-span-3"><span>内部备注</span><input v-model.trim="purchase.note" class="input" /></label>
                  </div></details>
                  <button type="button" class="btn" :disabled="busy" @click="buy">去淘宝购买</button>
                </div>
                <label class="check-line shipment-confirm"><input v-model="confirmed" type="checkbox" />{{ confirmationLabel }}</label>
              </fieldset>
              <p v-if="mode==='taobao'" class="muted">采购信息仅内部可见；确认发货只回传快递公司与单号。</p>
              <p v-if="uncertain" class="notice">上次发货结果待核查，请先核查状态。</p>
            </template>
            <p v-if="message" class="success-text" role="status">{{ message }}</p>
            <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
          </div>
          <footer class="source-editor-footer">
            <button type="button" class="btn ghost" :disabled="busy" @click="close">关闭</button>
            <button v-if="detail.kind!=='digital'" type="submit" class="btn" :disabled="busy || uncertain || !confirmed || (mode==='manual' && !canShip)">{{ mode==='manual'?'保存物流':'保存关联' }}</button>
            <button v-if="detail.kind!=='digital'" type="button" class="btn primary" :disabled="busy || !canShip || !confirmed || !logisticsVisible" @click="save(true)">{{ busy?'处理中…':'确认发货' }}</button>
          </footer>
        </form>
      </section>
    </div>
  </Teleport>
</template>
