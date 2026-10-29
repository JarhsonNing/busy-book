<script setup>
import {ref,watch} from 'vue';
const props=defineProps({url:{type:String,required:true},busy:Boolean,error:String});
const emit=defineEmits(['settings','stop']);
const loaded=ref(false),revision=ref(0);
watch(()=>props.url,()=>{loaded.value=false;});
function reload(){loaded.value=false;revision.value++;}
</script>
<template>
    <section class="opencode-web" aria-label="OpenCode 工作区">
      <header class="opencode-web-bar">
        <span class="tag success">{{ loaded?'运行中':'连接中' }}</span>
        <div class="spacer"></div>
        <button class="btn ghost" :disabled="busy" @click="emit('settings')">配置</button>
        <button class="btn ghost" :disabled="busy" @click="reload">重新加载</button>
        <button class="btn" :disabled="busy" @click="emit('stop')">{{ busy?'停止中…':'停止服务' }}</button>
      </header>
      <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
      <p v-if="!loaded" class="opencode-web-loading" role="status">正在连接 OpenCode…</p>
      <iframe :key="revision" :src="url" class="opencode-web-frame" title="OpenCode 网页工作区" tabindex="0" referrerpolicy="no-referrer" allow="clipboard-read; clipboard-write" @load="loaded=true"></iframe>
    </section>
</template>
