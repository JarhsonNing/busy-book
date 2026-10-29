<script setup>
import {computed,onMounted,onBeforeUnmount,reactive,ref} from 'vue';
import {api} from '../../api';
import AiGenerateDialog from './AiGenerateDialog.vue';
const props=defineProps({bookId:Number});
const emit=defineEmits(['updated','settings','progress']);
const data=ref(null),job=ref(null),open=ref(false),busy=ref(false),error=ref(''),saved=ref(''),message=ref('');
const form=reactive({title:'',description:'',text:'',sellPrice:null});
const dirty=computed(()=>JSON.stringify(form)!==saved.value);
const running=computed(()=>['queued','running'].includes(job.value?.state));
let timer,closed=false,lastRevision,loaded=false;
async function load(){
  try{
    const result=await api.aiDraft(props.bookId);if(closed)return;
    data.value=result;job.value=result.job;error.value='';
    if(!dirty.value || !saved.value){for(const key of Object.keys(form))form[key]=result.draft[key] ?? (key==='sellPrice'?null:'');saved.value=JSON.stringify(form);}
    if(loaded && lastRevision!==result.draft.revision)emit('updated');lastRevision=result.draft.revision;loaded=true;
  }catch(e){if(!closed)error.value=e.message;}
  clearTimeout(timer);if(!closed && (running.value || error.value))timer=setTimeout(load,2000);
}
async function save(){busy.value=true;error.value='';message.value='';try{await api.saveAiDraft(props.bookId,{...form,revision:data.value.draft.revision});saved.value=JSON.stringify(form);await load();emit('updated');message.value='草稿已保存';}catch(e){error.value=e.message;}finally{busy.value=false;}}
async function copyText(){try{await navigator.clipboard.writeText(form.text);message.value='文本已复制';}catch(e){error.value=e.message;}}
function created(value){job.value=value;message.value='任务已创建';load();}
function show(){open.value=true;}
defineExpose({show,refresh:load});
onMounted(load);onBeforeUnmount(()=>{closed=true;clearTimeout(timer);});
const money=n=>n==null?'待核实':'¥'+Number(n).toFixed(2);
</script>
<template>
  <section class="book-ai-draft detail-section">
    <header class="module-bar"><h3>销售草稿</h3><button class="btn small" @click="show">AI 完善</button></header>
    <p v-if="running" role="status" class="status-line">{{ job.stage || 'AI 排队中' }} <button class="link-btn" @click="emit('progress',job.id)">查看进度</button></p>
    <p v-if="job && ['failed','partial'].includes(job.state) && job.itemState!=='done'" class="error-panel" role="alert">{{ job.itemError || job.error || 'AI 生成失败，请查看任务' }} <button class="link-btn" @click="show">重新生成</button><button class="link-btn" @click="emit('settings')">模型设置</button></p>
    <p v-if="error" class="error-panel" role="alert">{{ error }} <button class="link-btn" @click="load">重试</button></p>
    <template v-if="data?.draft?.revision">
      <p class="muted ai-model-line">{{ data.draft.provider==='qoder'?'Qoder CLI':'OpenCode' }} · {{ data.draft.model || '默认模型' }}</p>
      <p v-if="data.draft.stale" class="status-line">资料或货源已变化，请重新生成草稿。</p>
      <p v-for="(warning,index) in data.draft.warnings || []" :key="index" class="status-line">{{ warning }}</p>
      <form @submit.prevent="save">
        <div class="form-grid"><label class="field"><span>参考成本（不含运费）</span><output class="ai-cost">{{ money(data.costBasis.cost) }}</output></label><label class="field"><span>草稿售价</span><input v-model.number="form.sellPrice" class="input" type="number" min="0.01" step="0.01" /></label><label class="field wide"><span>上架标题</span><input v-model="form.title" class="input" maxlength="30" /></label><label class="field wide"><span>上架文案</span><textarea v-model="form.description" class="input" rows="4" maxlength="4000" /></label></div>
        <details v-if="form.text || data.draft.task==='text'" class="ai-analysis" :open="data.draft.task==='text'"><summary>自定义文本</summary><textarea v-model="form.text" aria-label="自定义文本" class="input" rows="4" maxlength="8000" /><button type="button" class="link-btn" @click="copyText">复制文本</button></details>
        <details v-if="data.draft.analysis" class="ai-analysis"><summary>定价与生成依据</summary><p>{{ data.draft.analysis }}</p><p v-if="data.costBasis" class="muted">{{ data.costBasis.reason }} · 运费 {{ money(data.costBasis.freight) }}</p></details>
        <div class="row"><button class="btn" :disabled="busy || !dirty">保存草稿</button><span v-if="message" class="success-text" role="status">{{ message }}</span></div>
      </form>
    </template>
    <p v-else-if="!running && !error" class="muted">参考成本 {{ money(data?.costBasis?.cost) }} · 淘宝第二低商品价，不含运费</p>
    <AiGenerateDialog v-if="open" :ids="[bookId]" @close="open=false" @created="created" @settings="emit('settings')" />
  </section>
</template>
