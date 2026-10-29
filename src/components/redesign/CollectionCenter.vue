<script setup>
import {
  computed,
  nextTick,
  onMounted,
  onBeforeUnmount,
  reactive,
  ref,
  watch,
} from "vue";
import { api } from "../../api";
const props = defineProps({ active: Boolean });
const emit = defineEmits(["open-book", "work-item", "open-schools", "open-draft"]);
const jobs = ref([]),
  detailElement = ref(null),
  adapters = ref([]),
  selected = ref(null),
  creating = ref(false),
  error = ref(""),
  refreshError = ref(""),
  busy = ref(false),
  filter = ref("all");
const form = reactive({
  plugin: "",
  year: new Date().getFullYear() + 1,
  args: {},
  refresh: false,
});
const adapter = computed(() =>
  adapters.value.find((a) => a.ref === form.plugin),
);
const labels = {
  queued: "排队中",
  running: "运行中",
  waiting: "待处理",
  paused: "已暂停",
  completed: "已完成",
  partial: "部分失败",
  failed: "失败",
  canceled: "已取消",
};
const visible = computed(() =>
  jobs.value.filter(
    (j) =>
      filter.value === "all" ||
      (filter.value === "active"
        ? ["queued", "running", "waiting", "paused"].includes(j.state)
        : ["failed", "partial"].includes(j.state)),
  ),
);
let timer,
  run = 0;
async function refresh() {
  const version = ++run;
  try {
    const result = await api.commerce("/jobs");
    if (version !== run) return;
    jobs.value = result;
    refreshError.value = "";
    if (selected.value)
      selected.value =
        result.find((j) => j.id === selected.value.id) || selected.value;
  } catch (e) {
    if (version === run) refreshError.value = e.message;
  }
}
async function newTask() {
  error.value = "";
  try {
    const r = await api.commerce("/catalog/adapters");
    adapters.value = r.adapters.filter((a) => !a.error);
    form.plugin = adapters.value[0]?.ref || "";
    form.args = {};
    creating.value = true;
  } catch (e) {
    error.value = e.message;
  }
}
watch(
  () => form.plugin,
  () => {
    form.args = {};
    if (adapter.value?.key === "ecnu") form.args.school = "华东师范大学";
  },
);
async function create() {
  busy.value = true;
  error.value = "";
  try {
    const j = await api.commerce("/jobs", {
      kind: "catalog",
      ...form,
      title:
        (form.args.school || adapter.value?.name || "学校目录") +
        " · " +
        form.year,
    });
    creating.value = false;
    selected.value = j;
    await refresh();
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
async function control(j, action, body = {}) {
  busy.value = true;
  error.value = "";
  try {
    selected.value = await api.commerce(`/jobs/${j.id}/${action}`, body);
    await refresh();
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
async function selectJob(job){selected.value=job;await nextTick();detailElement.value?.scrollIntoView({block:'start'});}
async function openJob(id){try{await selectJob(await api.commerce('/jobs/'+id));}catch(e){error.value=e.message;}}
function tick() {
  clearInterval(timer);
  if (props.active) {
    refresh();
    timer = setInterval(() => {
      if (!document.hidden) refresh();
    }, 2000);
  }
}
watch(() => props.active, tick);
onMounted(tick);
onBeforeUnmount(() => {
  clearInterval(timer);
  run++;
});
defineExpose({ refresh, openJob });
const time = (v) =>
  new Date(v).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
</script>
<template>
  <section>
    <div class="module-bar">
      <nav class="segment" aria-label="任务筛选">
        <button
          v-for="(label, key) in {
            all: '全部任务',
            active: '进行中',
            failed: '异常',
          }"
          :key="key"
          :class="{ active: filter === key }"
          @click="filter = key"
        >
          {{ label }}
        </button>
      </nav>
      <button class="btn primary" @click="newTask">新建采集</button>
    </div>
    <p v-if="refreshError" class="error-panel" role="alert">{{ refreshError }}</p>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
    <div class="data-surface">
      <table class="data-table">
        <thead>
          <tr>
            <th>任务</th>
            <th>阶段</th>
            <th>进度</th>
            <th>成功 / 失败</th>
            <th>状态</th>
            <th>更新时间</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="j in visible" :key="j.id">
            <td>
              <button class="text-button" @click="selectJob(j)">
                {{ j.title }}
              </button>
            </td>
            <td>{{ j.stage || "等待执行" }}</td>
            <td>
              {{ j.processed }}{{ j.total != null ? " / " + j.total : " 页" }}
            </td>
            <td>
              {{
                j.result?.programs ??
                j.result?.items?.filter((i) => i.state === "done").length ??
                "—"
              }}
              / {{ j.result?.errors?.length ?? j.result?.items?.filter(i=>i.state==='failed').length ?? (j.state === "failed" ? 1 : 0) }}
            </td>
            <td>
              <span
                class="tag"
                :class="
                  j.state === 'completed'
                    ? 'success'
                    : ['failed', 'partial'].includes(j.state)
                      ? 'warning'
                      : 'neutral'
                "
                >{{ labels[j.state] }}</span
              >
            </td>
            <td class="muted">{{ time(j.updatedAt) }}</td>
            <td>
              <button class="btn small" @click="selectJob(j)">查看</button>
            </td>
          </tr>
          <tr v-if="!visible.length">
            <td colspan="7" class="table-empty">暂无采集任务</td>
          </tr>
        </tbody>
      </table>
    </div>
    <section v-if="selected" ref="detailElement" class="task-detail">
      <header class="module-bar">
        <h2>{{ selected.title }}</h2>
        <button class="btn small" @click="selected = null">收起</button>
      </header>
      <div class="row">
        <span class="tag">{{ labels[selected.state] }}</span
        ><span>{{ selected.stage }}</span>
        <span v-if="selected.kind==='ai'" class="muted">{{ selected.request.provider==='qoder'?'Qoder CLI':'OpenCode' }} · {{ selected.request.model || '默认模型' }}</span>
        <div class="push-right row">
          <button
            v-if="['queued', 'running', 'waiting'].includes(selected.state)"
            class="btn small"
            :disabled="busy"
            @click="control(selected, 'pause')"
          >
            暂停</button
          ><button
            v-if="selected.state === 'paused'"
            class="btn small"
            :disabled="busy"
            @click="control(selected, 'resume')"
          >
            继续</button
          ><button
            v-if="['failed', 'partial'].includes(selected.state)"
            class="btn small"
            :disabled="busy"
            @click="control(selected, 'retry')"
          >
            重试失败项</button
          ><button
            v-if="
              ['queued', 'running', 'waiting', 'paused'].includes(
                selected.state,
              )
            "
            class="btn small"
            :disabled="busy"
            @click="control(selected, 'cancel')"
          >
            取消任务
          </button>
        </div>
      </div>
      <p v-if="selected.error" class="error-panel">{{ selected.error }}</p>
      <table v-if="selected.result?.items" class="data-table">
        <tbody>
          <tr v-for="item in selected.result.items" :key="item.bookId">
            <td>
              <button
                class="text-button"
                @click="$emit('open-book', item.bookId)"
              >
                {{ item.title }}
              </button>
            </td>
            <td>{{ item.state === 'done' ? (selected.kind==='ai'?'草稿已保存':'已完成') : item.state==='failed'?'失败':'待处理' }}<small v-if="item.error" class="danger">{{ item.error }}</small></td>
            <td class="row">
              <button v-if="selected.kind==='ai' && item.state==='done'" class="btn small" @click="$emit('open-draft',item.bookId)">查看草稿</button>
              <button
                v-if="item.state !== 'done' && selected.kind!=='ai'"
                class="btn small"
                :disabled="selected.state !== 'waiting'"
                @click="$emit('work-item', selected.kind, item)"
              >
                {{
                  { images: "找图", sources: "找货源", publish: "准备发布" }[
                    selected.kind
                  ]
                }}</button
              ><button
                v-if="item.state !== 'done' && selected.kind!=='ai'"
                class="btn small"
                :disabled="busy || selected.state !== 'waiting'"
                @click="
                  control(selected, 'complete-item', { bookId: item.bookId })
                "
              >
                核验完成
              </button>
            </td>
          </tr>
        </tbody>
      </table>
      <template
        v-if="
          selected.kind === 'catalog' &&
          ['completed', 'partial'].includes(selected.state)
        "
        ><div class="result-line">
          <span>新增 {{ selected.result.import?.added || 0 }} 条用书</span
          ><span>变化 {{ selected.result.import?.changed || 0 }} 条</span
          ><button class="btn" @click="$emit('open-schools')">
            查看学校用书
          </button>
        </div>
        <ul v-if="selected.result.errors?.length" class="error-panel">
          <li v-for="(e, i) in selected.result.errors" :key="i">
            {{ typeof e === "string" ? e : JSON.stringify(e) }}
          </li>
        </ul></template
      >
    </section>
    <div v-if="creating" class="modal-backdrop" @click.self="creating = false">
      <section
        class="modal compact-dialog"
        v-modal-focus="
          () => {
            !busy && (creating = false);
          }
        "
        role="dialog"
        aria-modal="true"
        aria-labelledby="collect-title"
      >
        <header class="module-bar">
          <h2 id="collect-title">采集学校用书</h2>
          <button class="btn small" @click="creating = false">关闭</button>
        </header>
        <form @submit.prevent="create">
          <label class="field"
            ><span>来源类型</span
            ><SearchSelect v-model="form.plugin" class="input"
              ><option v-for="a in adapters" :key="a.ref" :value="a.ref">
                {{ a.name }}
              </option></SearchSelect
            ></label
          ><label class="field"
            ><span>年份</span
            ><input
              v-model.number="form.year"
              type="number"
              min="2000"
              max="2200"
              class="input"
              required /></label
          ><label
            v-for="field in (adapter?.fields || []).filter(
              (f) => !['slug', 'title'].includes(f.name),
            )"
            :key="field.name"
            class="field"
            ><span>{{ field.label }}</span
            ><input
              v-model="form.args[field.name]"
              class="input"
              :required="field.required"
              :placeholder="field.placeholder || ''" /></label
          ><label class="check-line"
            ><input
              type="checkbox"
              v-model="form.refresh"
            />重新获取已缓存页面</label
          >
          <p v-if="error" role="alert" class="error-panel">{{ error }}</p>
          <button class="btn primary" :disabled="busy || !form.plugin">
            开始采集
          </button>
        </form>
      </section>
    </div>
  </section>
</template>
