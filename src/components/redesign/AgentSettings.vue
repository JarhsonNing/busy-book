<script setup>
import {computed,onMounted,onBeforeUnmount,ref,watch} from 'vue';
import {api} from '../../api';
import QoderAccount from './QoderAccount.vue';
const props=defineProps({active:{type:Boolean,default:true},selectedProvider:{type:String,default:'opencode'}});
const emit=defineEmits(['open-opencode','open-qoder']);
const provider=ref(props.selectedProvider);
const name=computed(()=>provider.value==='qoder'?'Qoder CLI':'OpenCode');
const client=computed(()=>provider.value==='qoder'?{status:api.qoderStatus,select:api.selectQoder,browse:api.browseQoder,install:api.installQoder,service:api.qoderCli,stop:api.stopQoder}:{status:api.agentStatus,select:api.selectOpenCode,browse:api.browseOpenCode,install:api.installOpenCode,service:api.openCodeWeb,stop:api.stopOpenCodeWeb});
const info=ref(null),mcp=ref(null),busy=ref(false),error=ref(''),message=ref(''),path=ref(''),format=ref(props.selectedProvider);
const web=ref({running:false});
let timer,webTimer,webVersion=0,loadVersion=0,closed=false;
const installing=computed(()=>info.value?.install?.state==='running');
const local=computed(()=>info.value?.local || []);
const configText=computed(()=>mcp.value?JSON.stringify(mcp.value.configs[format.value],null,2):'');
const progress=computed(()=>{const job=info.value?.install;return job?.total?`${job.stage} · ${Math.round(job.downloaded/job.total*100)}%`:job?.stage || '';});
async function load() {
  const version=++loadVersion,currentClient=client.value;
  const [state,connection,service]=await Promise.all([currentClient.status(),api.agentMcp(),currentClient.service()]);
  if(closed || version!==loadVersion)return;info.value=state;mcp.value=connection;web.value=service;
  path.value=state.mode==='local'?(state.selectedPath || local.value[0]?.path || ''):(local.value.some(item=>item.path===path.value)?path.value:local.value[0]?.path || '');
  poll();
  watchWeb();
}
function watchWeb(){clearTimeout(webTimer);const version=webVersion;if(web.value.running&&!closed)webTimer=setTimeout(async()=>{try{const service=await client.value.service();if(closed||version!==webVersion)return;web.value=service;}catch(e){if(closed||version!==webVersion)return;error.value=e.message;}watchWeb();},5000);}
function poll(){clearTimeout(timer);if(installing.value&&!closed)timer=setTimeout(()=>load().catch(e=>{error.value=e.message;timer=setTimeout(poll,2500);}),1500);}
async function action(fn){if(busy.value)return;busy.value=true;error.value='';message.value='';try{await fn();}catch(e){error.value=e.message;}finally{busy.value=false;}}
async function choose(mode){await action(async()=>{info.value=await client.value.select({mode,path:mode==='local'?path.value:undefined});message.value='已选择';});}
async function browse(){await action(async()=>{const state=await client.value.browse();if(state.canceled)return;info.value=state;path.value=state.selectedPath;message.value=`已选择本机 ${name.value}`;});}
async function install(){await action(async()=>{if(info.value.mode!=='managed')info.value=await client.value.select({mode:'managed'});await client.value.install();info.value.install={state:'running',stage:'准备安装'};poll();});}
async function toggle(){await action(async()=>{mcp.value=await api.setAgentMcp(!mcp.value.enabled);message.value=mcp.value.enabled?'MCP 导入已开启':'MCP 导入已关闭';});}
async function copy(){await action(async()=>{mcp.value=await api.exportAgentMcp();await navigator.clipboard.writeText(configText.value);message.value='接入配置已复制';});}
async function stop(){if(!confirm(`停止 ${name.value}？当前任务会中断，未发送草稿会丢失。`))return;await action(async()=>{webVersion++;clearTimeout(webTimer);web.value=await client.value.stop();message.value=`${name.value} 已停止`;});watchWeb();}
async function prepare(){await action(async()=>{mcp.value=await api.exportAgentMcp();message.value='接入包与 Skill 已生成';});}
async function open(){if(provider.value==='qoder'){await action(async()=>{await api.launchQoder();emit('open-qoder');});}else emit('open-opencode');}
onMounted(()=>action(load));
watch(()=>props.active,(active)=>{if(active)action(load);});
watch(provider,()=>{webVersion++;clearTimeout(timer);clearTimeout(webTimer);info.value=null;web.value={running:false};path.value='';format.value=provider.value;action(load);});
onBeforeUnmount(()=>{closed=true;clearTimeout(timer);clearTimeout(webTimer);});
</script>
<template>
  <section class="agent-settings settings-form">
    <nav class="sub-tabs" aria-label="Agent 程序"><button v-for="(label,key) in {opencode:'OpenCode',qoder:'Qoder CLI'}" :key="key" :class="{active:provider===key}" :disabled="busy" :aria-pressed="provider===key" @click="provider=key">{{ label }}</button></nav>
    <p v-if="error" class="error-panel" role="alert">{{ error }} <button class="link-btn" :disabled="busy" @click="action(load)">重试</button></p>
    <p v-if="message" class="success-text" role="status">{{ message }}</p>
    <template v-if="info && mcp">
      <div class="module-bar"><h2>{{ name }}</h2><button class="btn small ghost" :disabled="busy || installing" @click="action(load)">重新检测</button></div>
      <QoderAccount v-if="provider==='qoder' && info.selectedPath" />
      <div class="agent-options">
        <label class="check-line"><input type="radio" name="opencode-mode" :checked="info.mode==='local'" :disabled="busy || web.running" @change="choose('local')" />使用本机安装</label>
        <label class="check-line"><input type="radio" name="opencode-mode" :checked="info.mode==='managed'" :disabled="busy || web.running" @change="choose('managed')" />使用应用专用版</label>
      </div>
      <div v-if="info.mode==='local'" class="agent-choice">
        <label class="field"><span>本机程序</span><select v-model="path" class="input" :disabled="busy || web.running || !local.length" @change="choose('local')"><option v-if="!local.length" value="">未检测到 {{ name }}</option><option v-for="item in local" :key="item.path" :value="item.path">{{ item.available?item.version:'不可用' }} · {{ item.path }}</option></select></label>
        <button class="btn" :disabled="busy || web.running" @click="browse">选择程序</button>
        <button v-if="!local.some(item=>item.available)" class="btn" :disabled="busy || installing" @click="install">安装专用版</button>
      </div>
      <div v-else class="agent-managed">
        <div class="row"><span class="tag" :class="info.managed.installed?'success':'neutral'">{{ info.managed.installed?'已安装':'未安装' }}</span><span>{{ name }} {{ info.managed.version || info.install.version }}</span><button class="btn" :disabled="busy || installing || info.managed.installed" @click="install">{{ installing?'安装中…':info.managed.installed?'已安装':'安装专用版' }}</button></div>
        <p class="muted">{{ provider==='qoder'?'使用官方原生发行包，沿用本机 Qoder 账号登录。':'程序与配置独立存放，不修改本机 OpenCode。' }}</p>
      </div>
      <p v-if="installing" class="status-line" role="status">{{ progress }}</p>
      <p v-if="info.install.state==='failed'" class="error-panel" role="alert">安装失败：{{ info.install.error }} <button class="link-btn" :disabled="busy || installing" @click="install">重试安装</button></p>
      <div class="agent-launch"><button class="btn primary" :disabled="busy || !info.selectedPath || !mcp.enabled" @click="open">{{ web.running?'进入':'打开' }} {{ name }}</button><button v-if="web.running" class="btn ghost" :disabled="busy" @click="stop">停止运行</button><span class="muted">{{ !mcp.enabled?'请先开启 MCP 导入':provider==='qoder'?'在原生终端中交互；首次启动按提示登录':web.running?'后台运行中；停止后可切换程序':info.mode==='local'?'沿用本机模型配置':'专用版使用独立模型配置' }}</span></div>
      <label class="field agent-workspace"><span>固定项目目录 · 两个 Agent 共用</span><input class="input" :value="info.workspace" readonly /></label>
      <section class="agent-mcp">
        <div class="module-bar"><h2>MCP 本地导入</h2><span class="tag" :class="mcp.enabled?'success':'neutral'">{{ mcp.enabled?'已开启':'未开启' }}</span><button class="btn small" :disabled="busy" @click="toggle">{{ mcp.enabled?'关闭导入':'开启导入' }}</button></div>
        <p>图书、学校用书、货源、图片 · 已导入 {{ mcp.importCount }} 条</p>
        <div class="row agent-export"><label class="field"><span>接入格式</span><select v-model="format" class="input"><option value="opencode">OpenCode v2</option><option value="qoder">Qoder CLI</option><option value="stdio">通用 MCP 客户端</option></select></label><button class="btn" :disabled="busy" @click="copy">复制接入配置</button><button class="btn ghost" :disabled="busy" @click="prepare">生成接入包与 Skill</button></div>
        <details class="agent-config"><summary>配置与文件位置</summary><pre>{{ configText }}</pre><p class="muted">工作目录</p><code>{{ mcp.configs.workspace }}</code><p class="muted">Skill</p><code>{{ provider==='qoder'?mcp.configs.qoderSkillPath:mcp.configs.skillPath }}</code><p class="muted">应用运行期间可导入；关闭导入后连接立即失效。</p></details>
      </section>
    </template>
    <p v-else-if="!error" role="status" class="muted">正在检测 {{ name }}…</p>
  </section>
</template>
