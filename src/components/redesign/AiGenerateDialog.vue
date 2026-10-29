<script setup>
import {onMounted,ref,watch} from 'vue';
import {X} from '@lucide/vue';
import {api} from '../../api';
import AiModelSelect from './AiModelSelect.vue';
import QoderAccount from './QoderAccount.vue';
const props=defineProps({ids:{type:Array,required:true},initialTask:{type:String,default:'complete'},sourceSearch:Boolean});
const emit=defineEmits(['close','created','settings','search-started']);
const config=ref(null),task=ref(props.initialTask),prompt=ref(''),busy=ref(false),error=ref(''),message=ref('');
const modelSelect=ref(null);
function close(){if(!busy.value)emit('close');}
async function load(){try{config.value=await api.aiConfig();prompt.value=config.value.prompts[task.value];}catch(e){error.value=e.message;}}
watch(task,t=>{if(config.value)prompt.value=config.value.prompts[t];});
function chooseProvider(event){const next=event.target.value;config.value.models[config.value.provider]=config.value.model;config.value.provider=next;config.value.model=config.value.models[next] || '';}
async function generate(){
  if(busy.value)return;busy.value=true;error.value='';
  try{
    const input={task:task.value,prompt:prompt.value,provider:config.value.provider,model:config.value.model};
    if(props.sourceSearch){const search=await api.commerce(`/books/${props.ids[0]}/price-search`,{action:'start',ai:input});emit('search-started',search);}
    else{const job=await api.commerce('/jobs',{kind:'ai',bookIds:props.ids,...input});emit('created',job);}
    emit('close');
  }
  catch(e){error.value=e.message;}finally{busy.value=false;}
}
async function copyCommand(){
  error.value='';message.value='';
  try{const value=await api.aiCommand(props.ids[0],task.value,prompt.value,{provider:config.value.provider,model:config.value.model});await navigator.clipboard.writeText(value.command);message.value='命令已复制，包含本次执行器、模型和提示词';}
  catch(e){error.value=e.message;}
}
onMounted(load);
</script>
<template>
  <Teleport to="body"><div class="modal-backdrop" @click.self="close">
    <section v-modal-focus="close" class="modal-dialog ai-generate-dialog" role="dialog" aria-modal="true" aria-labelledby="ai-generate-title" :aria-busy="busy">
      <header class="delete-books-heading"><h2 id="ai-generate-title">{{ initialTask==='sources' ? (sourceSearch?'搜索并整理货源':'AI 整理货源') : 'AI 完善 · '+ids.length+' 本' }}</h2><button class="icon-btn" aria-label="关闭 AI 生成" :disabled="busy" @click="close"><X :size="18"/></button></header>
      <p v-if="!config && !error" role="status">正在读取提示词…</p>
      <template v-if="config">
        <label v-if="initialTask!=='sources'" class="field"><span>任务</span><SearchSelect v-model="task" class="input" aria-label="AI 任务"><option value="complete">定价并生成文案</option><option value="pricing">分析定价</option><option value="copy">生成上架文案</option><option value="text">自定义文本</option></SearchSelect></label>
        <label class="field"><span>本次执行器</span><select class="input" :value="config.provider" :disabled="busy" @change="chooseProvider"><option value="opencode">OpenCode</option><option value="qoder">Qoder CLI</option></select></label>
        <QoderAccount v-if="config.provider==='qoder'" @signed-in="modelSelect?.refresh()" />
        <AiModelSelect ref="modelSelect" v-model="config.model" :provider="config.provider" :disabled="busy" />
        <p class="muted ai-model-line"><button class="link-btn" :disabled="busy" @click="emit('settings');emit('close')">默认执行器与提示词设置</button></p>
        <details class="ai-prompt"><summary>本次提示词</summary><textarea v-model="prompt" aria-label="本次提示词" class="input" rows="6" maxlength="12000" :disabled="busy" /></details>
        <p class="muted ai-save-note">{{ initialTask==='sources' ? (sourceSearch?'打开淘宝搜索，采集结束后自动使用本次模型整理；遇到登录或验证时可继续检索。':'根据已采集的资料逐条比较，保存推荐、备用和待核实项。仅有链接的货源会标记资料待补充。') : '结果自动写入草稿，发布前确认。' }}</p>
        <p v-if="initialTask==='sources'" class="muted">整理使用所选模型额度；主选货源由你核对后保存。</p>
      </template>
      <p v-if="error" class="error-panel" role="alert">{{ error }} <button v-if="!config" class="link-btn" @click="load">重试</button><button class="link-btn" @click="emit('settings');emit('close')">打开设置</button></p>
      <p v-if="message" class="success-text" role="status">{{ message }}</p>
      <footer class="modal-actions"><button v-if="ids.length===1 && !sourceSearch" class="btn ghost" :disabled="!config || busy" @click="copyCommand">复制命令</button><button class="btn push-right" :disabled="busy" @click="close">取消</button><button class="btn primary" :disabled="!config || busy || !prompt.trim()" @click="generate">{{ busy?'创建中…':initialTask==='sources'?(sourceSearch?'开始搜索':'开始整理'):'生成并写入草稿' }}</button></footer>
    </section>
  </div></Teleport>
</template>
