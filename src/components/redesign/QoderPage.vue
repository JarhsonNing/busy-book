<script setup>
import {onMounted,onBeforeUnmount,ref,watch} from 'vue';
import {api} from '../../api';
import QoderAccount from './QoderAccount.vue';
const props=defineProps({active:Boolean});
const emit=defineEmits(['settings','ai-settings']);
const state=ref({running:false}),info=ref(null),busy=ref(false),error=ref('');
let timer,closed=false;
async function load(refresh=true){
  clearTimeout(timer);
  try{const [service,config]=await Promise.all([api.qoderCli(),refresh?api.qoderStatus():Promise.resolve(info.value)]);if(closed)return;state.value=service;info.value=config;}
  catch(e){if(!closed)error.value=e.message;}
  if(!closed&&props.active)timer=setTimeout(()=>load(false),5000);
}
async function launch(){busy.value=true;error.value='';try{state.value=await api.launchQoder();}catch(e){error.value=e.message;}finally{busy.value=false;}}
async function stop(){if(!confirm('关闭 Qoder 终端？当前任务会中断，未发送内容会丢失。'))return;busy.value=true;error.value='';try{state.value=await api.stopQoder();}catch(e){error.value=e.message;}finally{busy.value=false;}}
watch(()=>props.active,active=>{if(active)load();else clearTimeout(timer);});
onMounted(load);
onBeforeUnmount(()=>{closed=true;clearTimeout(timer);});
</script>
<template>
  <section class="settings-form agent-settings" :aria-busy="busy">
    <div class="module-bar"><h2>Qoder CLI</h2><span class="tag" :class="state.running?'success':'neutral'">{{ state.running?'终端运行中':'未运行' }}</span></div>
    <p>在原生终端中与 Qoder 交互。首次启动时，按终端提示登录。</p>
    <QoderAccount v-if="info?.selectedPath" />
    <p class="muted">终端沿用 AI 流程中保存的 Qoder 模型，重新打开终端后生效。<button class="link-btn" @click="emit('ai-settings')">模型设置</button></p>
    <label v-if="info" class="field"><span>固定项目目录 · 与 OpenCode 共用</span><input class="input" :value="info.workspace" readonly /></label>
    <p v-if="state.running" role="status" class="muted">请切换到已打开的 Qoder 终端。切换本应用菜单会保留终端，退出应用时终端关闭。</p>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
    <div class="row"><button class="btn primary" :disabled="busy || state.running || !info?.selectedPath" @click="launch">{{ busy?'处理中…':state.running?'终端已打开':'打开 Qoder CLI' }}</button><button v-if="state.running" class="btn" :disabled="busy" @click="stop">关闭终端</button><button class="btn ghost" :disabled="busy" @click="emit('settings')">安装与接入设置</button></div>
    <p v-if="info && !info.selectedPath" class="muted">请先在接入设置中安装 Qoder CLI 或选择本机程序。</p>
  </section>
</template>
