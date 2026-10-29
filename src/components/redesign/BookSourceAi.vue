<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { api, openBrowser } from '../../api';
import { showTime } from '../../commerce';
import AiGenerateDialog from './AiGenerateDialog.vue';

const props = defineProps({ bookId: Number, search: Object });
const emit = defineEmits(['updated', 'edit-source', 'search-started', 'settings', 'progress', 'add-source']);
const data = ref(null), error = ref(''), loadError = ref(''), message = ref(''), busy = ref(false);
const dialog = ref(false), searchMode = ref(false), linksOpen = ref(false), links = ref('');
const report = computed(() => data.value?.report);
const job = computed(() => data.value?.job);
const running = computed(() => ['queued', 'running', 'paused'].includes(job.value?.state));
const searching = computed(() => ['searching', 'waiting'].includes(props.search?.state));
const rows = computed(() => (report.value?.sources || []).map(row => ({
  ...row, source: data.value?.sources.find(s => s.id === row.sourceId),
})).sort((a, b) => rank(a) - rank(b)).filter(r => r.source));
const money = v => v == null ? '待核实' : '¥' + Number(v).toFixed(2);
const verdicts = { match: '版本匹配', mismatch: '版本不符', unknown: '版本待核实' };
function rank(row) {
  if (row.sourceId === report.value?.recommendedSourceId) return 0;
  if (report.value?.backupSourceIds?.includes(row.sourceId)) return 1;
  return row.versionMatch === 'mismatch' ? 3 : 2;
}
function role(row) {
  if (row.sourceId === report.value?.recommendedSourceId) return '推荐';
  if (report.value?.backupSourceIds?.includes(row.sourceId)) return '备用';
  return '';
}
function missing(s) {
  return [!s.sku && '规格', !s.condition && '品相', s.price == null && '采购价',
    s.freight == null && '运费', !s.dropship && '代发',
    (!s.checkedAt || s.status !== 'available') && '人工核验'].filter(Boolean).join('、');
}
let timer, closed = false, version = 0;
async function load() {
  const current = ++version, id = props.bookId;
  clearTimeout(timer);
  try {
    const value = await api.sourceAnalysis(id);
    if (closed || current !== version || id !== props.bookId) return;
    data.value = value; loadError.value = '';
  } catch (e) { if (!closed && current === version) loadError.value = e.message; }
  if (!closed && current === version && (running.value || searching.value || loadError.value))
    timer = setTimeout(load, loadError.value ? 5000 : 2000);
}
function generate(search) { searchMode.value = search; dialog.value = true; }
async function created() { await load(); }
async function addLinks() {
  if (busy.value) return;
  busy.value = true; error.value = ''; message.value = '';
  const id = props.bookId;
  try {
    const result = await api.commerce(`/books/${id}/source-links`, { urls: links.value.trim().split(/\s+/).filter(Boolean) });
    if (closed || id !== props.bookId) return;
    message.value = `已添加 ${result.added} 个货源${result.existing ? '，跳过 '+result.existing+' 个重复链接' : ''}。打开商品采集详情后再整理。`;
    links.value = ''; linksOpen.value = false;
    await load(); emit('updated');
  } catch (e) { error.value = e.message; }
  finally { busy.value = false; }
}
async function openSource(s) {
  error.value = '';
  try {
    const url = new URL(s.url); url.searchParams.set('taoshuSourceBook', String(props.bookId));
    await openBrowser('taobao', url.href);
  } catch (e) { error.value = e.message; }
}
function edit(row, primary = false) {
  const s = row.source;
  emit('edit-source', { id: s.id, primary: s.primary, data: s }, primary);
}
async function resume() {
  error.value = '';
  try { await api.commerce(`/jobs/${job.value.id}/resume`, {}); await load(); }
  catch (e) { error.value = e.message; }
}
watch(() => props.bookId, () => { data.value = null; links.value = ''; linksOpen.value = false; dialog.value = false; message.value = ''; error.value = ''; loadError.value = ''; load(); }, { immediate: true });
watch(() => [props.search?.state, props.search?.analysisJobId], load);
onMounted(() => window.addEventListener('focus', load));
onBeforeUnmount(() => { closed = true; version++; clearTimeout(timer); window.removeEventListener('focus', load); });
defineExpose({ refresh: load });
</script>

<template>
  <section class="source-ai" aria-labelledby="source-ai-title">
    <div class="module-bar">
      <h3 id="source-ai-title">AI 整理货源</h3>
      <div class="row source-ai-actions">
        <button class="btn primary" :disabled="running || searching" @click="generate(true)">搜索并用 AI 整理</button>
        <button class="btn" :disabled="running || searching || !data?.sources?.length" @click="generate(false)">整理已有货源</button>
        <button class="btn ghost" :aria-expanded="linksOpen" @click="linksOpen = !linksOpen">添加链接</button>
        <button class="btn ghost" @click="emit('add-source')">手动录入</button>
      </div>
    </div>
    <form v-if="linksOpen" class="source-links-form" @submit.prevent="addLinks">
      <label class="field"><span>淘宝 / 天猫商品链接</span><textarea v-model="links" class="input" rows="3" required :disabled="busy" placeholder="每行一个完整商品链接，最多 20 个" /></label>
      <div class="row"><button class="btn" :disabled="busy || !links.trim()">{{ busy ? '添加中…' : '加入货源候选' }}</button><span class="muted">添加后可打开商品采集详情，或手动补充资料。</span></div>
    </form>
    <p v-if="message" class="success-text" role="status">{{ message }}</p>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
    <p v-if="loadError" class="error-panel" role="alert">{{ loadError }} <button class="link-btn" @click="load">重新读取</button></p>
    <p v-if="searching && search.aiRequest" class="status-line" role="status">{{ search.state==='waiting'?'搜索待继续，候选已保留':'正在采集淘宝候选，结束后自动整理' }} · {{ search.count || 0 }} 个候选</p>
    <p v-if="search?.analysisError" class="error-panel" role="alert">{{ search.analysisError }}，可调整候选后点击「整理已有货源」。</p>
    <p v-if="running" class="status-line" role="status">{{ job.state==='paused'?'货源整理已暂停':job.stage || 'AI 整理排队中' }} <button v-if="job.state==='paused'" class="link-btn" @click="resume">继续整理</button><button class="link-btn" @click="emit('progress', job.id)">查看任务</button></p>
    <p v-if="job && ['failed','partial','canceled'].includes(job.state)" class="error-panel" role="alert">{{ job.error || (job.state==='canceled'?'本次整理已取消':'本次整理未完成') }} <button class="link-btn" :disabled="searching" @click="generate(false)">重新整理</button><button class="link-btn" @click="emit('settings')">模型设置</button></p>
    <template v-if="report">
      <div class="source-ai-conclusion">
        <div class="row"><strong>整理结论</strong><span v-if="report.stale" class="tag warning">资料已变化</span><span v-else-if="report.recommendedSourceId==null" class="tag warning">暂无合适推荐</span></div>
        <p>{{ report.analysis }}</p>
        <p v-if="report.stale" class="status-line">以下是上次结果，请重新整理后再确认推荐。</p>
        <small class="muted">{{ report.provider==='qoder'?'Qoder CLI':'OpenCode' }} · {{ report.model || '默认模型' }} · {{ showTime(report.generatedAt) }} · AI 判断供核对</small>
      </div>
      <p class="source-ai-scroll-note muted">左右滚动查看完整比较和操作。</p>
      <div class="source-ai-table" tabindex="0" role="region" aria-label="货源比较，可横向滚动">
        <table class="data-table"><thead><tr><th>货源 / AI 版本判断</th><th>价格与条件</th><th>推荐依据与待核实项</th><th>操作</th></tr></thead>
          <tbody><tr v-for="row in rows" :key="row.sourceId">
            <td><div class="row"><span v-if="role(row)" class="tag" :class="report.stale?'neutral':'success'">{{ report.stale?'上次':'' }}{{ role(row) }}</span><strong>{{ row.source.shop || row.source.title || '待采集商品' }}</strong><span v-if="row.source.primary" class="tag neutral">当前主选</span></div><small v-if="row.source.shop && row.source.title">{{ row.source.title }}</small><small>{{ row.source.sku || '规格待补充' }} · {{ row.source.condition || '品相待核实' }}</small><span class="tag" :class="row.versionMatch==='mismatch'?'danger':row.versionMatch==='match'?'neutral':'warning'">{{ verdicts[row.versionMatch] }}</span></td>
            <td><span>{{ row.source.status==='available'?'采购价':'参考价' }} {{ money(row.source.price) }}</span><small>运费 {{ money(row.source.freight) }}</small><small>{{ row.source.price!=null && row.source.freight!=null ? (row.source.status==='available'?'已核验合计 ':'参考合计 ')+money(row.source.price+row.source.freight) : '到手价待核实' }}</small><small>{{ row.source.dropship?'已登记支持代发':'代发待确认' }}</small></td>
            <td><p>{{ row.reason }}</p><ul v-if="row.risks.length" class="source-ai-risks"><li v-for="(risk,index) in row.risks" :key="index">{{ risk }}</li></ul><small v-if="missing(row.source)" class="muted">待补充：{{ missing(row.source) }}</small></td>
            <td><div class="source-ai-row-actions"><button class="btn small" @click="edit(row)">核对资料</button><button v-if="role(row) && !row.source.primary" class="btn small" :disabled="report.stale || running || searching || row.source.status==='unavailable'" @click="edit(row,true)">核对并设主选</button><button class="link-btn" @click="openSource(row.source)">打开商品采集</button></div></td>
          </tr></tbody>
        </table>
      </div>
    </template>
    <p v-else-if="!running && !searching && !error" class="muted source-ai-empty">搜索淘宝货源，或添加已有链接；AI 会比较版本、价格与代发条件，给出推荐和备用。</p>
    <AiGenerateDialog v-if="dialog" :ids="[bookId]" initial-task="sources" :source-search="searchMode" @close="dialog=false" @created="created" @search-started="emit('search-started',$event)" @settings="emit('settings')" />
  </section>
</template>
