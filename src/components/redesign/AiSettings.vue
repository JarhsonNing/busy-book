<script setup>
import {computed,onMounted,ref} from 'vue';
import {api} from '../../api';
import AiModelSelect from './AiModelSelect.vue';
import QoderAccount from './QoderAccount.vue';
const emit=defineEmits(['agent-settings']);
const form=ref(null),defaults=ref({}),task=ref('complete'),busy=ref(false),error=ref(''),message=ref(''),saved=ref('');
const modelSelect=ref(null);
const dirty=computed(()=>JSON.stringify(form.value)!==saved.value);
async function load(){error.value='';try{const v=await api.aiConfig();defaults.value=v.defaultPrompts || {};delete v.defaultPrompts;form.value=v;saved.value=JSON.stringify(v);}catch(e){error.value=e.message;}}
function chooseProvider(event){const next=event.target.value;form.value.models[form.value.provider]=form.value.model;form.value.provider=next;form.value.model=form.value.models[next] || '';}
async function save(){busy.value=true;error.value='';message.value='';try{form.value=await api.saveAiConfig(form.value);saved.value=JSON.stringify(form.value);message.value='AI 设置已保存';}catch(e){error.value=e.message;}finally{busy.value=false;}}
onMounted(load);
</script>
<template>
  <form v-if="form" class="settings-form ai-settings" @submit.prevent="save">
    <div class="module-bar"><h2>AI 流程</h2><button class="btn small ghost" type="button" @click="emit('agent-settings',form.provider)">配置 {{ form.provider==='qoder'?'Qoder CLI':'OpenCode' }}</button></div>
    <label class="field"><span>AI 执行器</span><select class="input" :value="form.provider" :disabled="busy" @change="chooseProvider"><option value="opencode">OpenCode</option><option value="qoder">Qoder CLI</option></select></label>
    <QoderAccount v-if="form.provider==='qoder'" @signed-in="modelSelect?.refresh()" />
    <AiModelSelect ref="modelSelect" v-model="form.model" :provider="form.provider" :disabled="busy" />
    <p class="muted ai-model-line">成本取淘宝商品价的第二低价，去重、不含运费。</p>
    <label class="check-line"><input v-model="form.enabled" type="checkbox" />自动完善草稿</label>
    <div v-if="form.enabled" class="ai-auto-options"><label class="check-line"><input v-model="form.autoOnImport" type="checkbox" />图书入库或资料变更后</label><label class="check-line"><input v-model="form.autoOnSources" type="checkbox" />货源更新后</label><p class="muted">使用所选模型额度；结果写入草稿，发布前确认。</p></div>
    <label class="field"><span>提示词模板</span><SearchSelect v-model="task" class="input" aria-label="提示词模板"><option value="sources">整理货源</option><option value="complete">定价并生成文案</option><option value="pricing">分析定价</option><option value="copy">生成上架文案</option><option value="text">自定义文本</option></SearchSelect></label>
    <label class="field"><span>提示词</span><textarea v-model="form.prompts[task]" class="input" rows="7" maxlength="12000" required /></label>
    <div class="row"><button class="btn primary" :disabled="busy || !dirty">保存设置</button><button v-if="defaults[task]" class="btn ghost" type="button" :disabled="busy" @click="form.prompts[task]=defaults[task]">恢复当前默认提示词</button></div>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p><p v-if="message" class="success-text" role="status">{{ message }}</p>
  </form>
  <p v-else class="error-panel" role="status">{{ error || '正在读取 AI 设置…' }} <button v-if="error" class="link-btn" @click="load">重试</button></p>
</template>
