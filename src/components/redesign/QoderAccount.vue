<script setup>
import {onMounted,onBeforeUnmount,ref} from 'vue';
import {api} from '../../api';
const loggedIn=ref(null),loading=ref(false),busy=ref(false),error=ref(''),message=ref('');
const emit=defineEmits(['signed-in']);
let timer,closed=false;
async function refresh(){loading.value=true;error.value='';try{const value=await api.qoderAccount();if(!closed){const wasLoggedIn=loggedIn.value;loggedIn.value=value.loggedIn;if(value.loggedIn===true&&wasLoggedIn===false)emit('signed-in');}}catch(e){if(!closed)error.value=e.message;}finally{loading.value=false;}}
async function watchLogin(){
  if(closed)return;
  try{const terminal=await api.qoderCli();if(!terminal.running){await refresh();return;}}
  catch(e){if(!closed)error.value=e.message;return;}
  if(!closed)timer=setTimeout(watchLogin,2000);
}
async function login(){busy.value=true;error.value='';message.value='';try{await api.loginQoder();message.value='已打开 Qoder 登录窗口，请在终端与浏览器中完成登录。';clearTimeout(timer);if(!closed)timer=setTimeout(watchLogin,2000);}catch(e){error.value=e.message;}finally{busy.value=false;}}
onMounted(refresh);onBeforeUnmount(()=>{closed=true;clearTimeout(timer);});
</script>
<template>
  <section class="qoder-account" aria-label="Qoder 账号">
    <div class="row"><span class="tag" :class="loggedIn===true?'success':'neutral'">{{ loading?'正在读取登录状态…':loggedIn===true?'Qoder 已登录':loggedIn===false?'Qoder 未登录':'Qoder 登录状态未知' }}</span><button type="button" class="btn small" :disabled="busy || loading" @click="login">{{ busy?'正在打开…':loggedIn?'重新登录 Qoder':'登录 Qoder' }}</button><button type="button" class="btn small ghost" :disabled="loading || busy" @click="refresh">刷新登录状态</button></div>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p><p v-if="message" class="muted" role="status">{{ message }}</p>
  </section>
</template>
