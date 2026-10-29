<script setup>
import {onMounted,onBeforeUnmount,ref,watch} from 'vue';
import {api} from '../../api';
const props=defineProps({provider:{type:String,default:'opencode'},modelValue:{type:String,default:''},disabled:Boolean});
const emit=defineEmits(['update:modelValue']);
const models=ref([]),busy=ref(false),error=ref('');
let version=0,closed=false;
async function load(){const request=++version;busy.value=true;error.value='';try{const result=await api.aiModels(props.provider);if(!closed&&request===version)models.value=result.items;}catch(e){if(!closed&&request===version)error.value=e.message;}finally{if(request===version)busy.value=false;}}
defineExpose({refresh:load});
watch(()=>props.provider,()=>{models.value=[];load();});
onMounted(load);onBeforeUnmount(()=>{closed=true;version++;});
</script>
<template>
  <div class="field"><span>执行模型</span><div class="row"><SearchSelect :model-value="modelValue" :disabled="disabled" class="input" aria-label="AI 执行模型" @update:model-value="emit('update:modelValue',$event)"><option value="">沿用 {{ provider==='qoder'?'Qoder':'OpenCode' }} 默认模型</option><option v-if="modelValue && !models.includes(modelValue)" :value="modelValue">{{ modelValue }}</option><option v-for="item in models" :key="item" :value="item">{{ item }}</option></SearchSelect><button class="btn" type="button" :disabled="busy || disabled" @click="load">{{ busy?'读取中…':'刷新模型' }}</button></div>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
    <details v-if="provider==='qoder'" class="ai-custom-model"><summary>输入自定义模型编号</summary><label class="field"><span>Qoder 模型名称或完整 BYOK 编号</span><input class="input" :disabled="disabled" :value="modelValue" maxlength="200" @input="emit('update:modelValue',$event.target.value)" /></label><p class="muted">留空沿用 Qoder 默认模型；较长的 BYOK 编号请填写完整值。</p></details>
  </div>
</template>
