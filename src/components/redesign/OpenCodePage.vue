<script setup>
import {onMounted,onBeforeUnmount,ref,watch} from 'vue';
import {api} from '../../api';
import OpenCodeWeb from './OpenCodeWeb.vue';
const props=defineProps({active:Boolean});
const emit=defineEmits(['settings']);
const web=ref({running:false}),busy=ref(false),error=ref('');
let timer,closed=false,version=0;
function poll(){
  clearTimeout(timer);
  if(!web.value.running || closed)return;
  const current=version;
  timer=setTimeout(async()=>{
    try{
      const state=await api.openCodeWeb();
      if(closed || current!==version)return;
      web.value=state;
      error.value=state.running?'':'OpenCode 已退出，请重新打开';
    }catch(e){if(closed || current!==version)return;error.value=e.message;}
    poll();
  },5000);
}
async function launch(){
  if(busy.value)return;
  version++;clearTimeout(timer);
  busy.value=true;error.value='';
  try{const state=await api.launchOpenCode();if(!closed){web.value=state;poll();}}
  catch(e){if(!closed)error.value=e.message;}
  finally{busy.value=false;}
}
async function stop(){
  if(busy.value || !confirm('停止 OpenCode 服务？当前任务会中断，未发送草稿会丢失。'))return;
  version++;
  busy.value=true;error.value='';clearTimeout(timer);
  try{web.value=await api.stopOpenCodeWeb();}
  catch(e){error.value=e.message;}
  finally{busy.value=false;poll();}
}
watch(()=>props.active,(active)=>{if(active && !web.value.running)launch();});
onMounted(launch);
onBeforeUnmount(()=>{closed=true;clearTimeout(timer);});
</script>
<template>
  <section class="opencode-page" :aria-busy="busy">
    <OpenCodeWeb v-if="web.running" :url="web.url" :busy="busy" :error="error" @settings="emit('settings')" @stop="stop" />
    <div v-else class="empty-state">
      <p v-if="busy" role="status">正在启动 OpenCode…</p>
      <template v-else>
        <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
        <p v-else>OpenCode 未运行</p>
        <div class="row"><button class="btn primary" @click="launch">打开 OpenCode</button><button class="btn" @click="emit('settings')">打开设置</button></div>
      </template>
    </div>
  </section>
</template>
