<script setup>
import {
  computed,
  onBeforeUnmount,
  onMounted,
  reactive,
  ref,
  watch,
} from "vue";
import { Search, Plus, BookOpen } from "@lucide/vue";
import { api, assetUrl } from "../../api";
import SchoolBooks from "./SchoolBooks.vue";
const emit = defineEmits(["open-book", "publish", "batch", "collect"]);
const view = ref("books"),
  schoolView = ref(null),
  loading = ref(false),
  error = ref(""),
  data = ref({ items: [], total: 0, schools: [], years: [] }),
  selected = ref(new Set());
const filter = reactive({
  q: "",
  school: "",
  major: "",
  year: "",
  readiness: "",
  page: 1,
  pageSize: 30,
});
let requestId = 0,
  timer;
const pages = computed(() =>
  Math.max(1, Math.ceil(data.value.total / filter.pageSize)),
);
const allSelected = computed(
  () =>
    data.value.items.length > 0 &&
    data.value.items.every((b) => selected.value.has(b.id)),
);
function toggle(id) {
  selected.value.has(id) ? selected.value.delete(id) : selected.value.add(id);
}
function toggleAll() {
  const clear = allSelected.value;
  for (const b of data.value.items)
    clear ? selected.value.delete(b.id) : selected.value.add(b.id);
}
async function refresh() {
  const run = ++requestId;
  loading.value = true;
  error.value = "";
  try {
    const p = new URLSearchParams(
      Object.entries(filter).filter(([, v]) => v !== ""),
    );
    const r = await api.commerce("/library?" + p);
    if (run === requestId) {
      data.value = r;
      filter.page = r.page;
      selected.value = new Set(
        [...selected.value].filter((id) => r.items.some((b) => b.id === id)),
      );
    }
  } catch (e) {
    if (run === requestId) error.value = e.message;
  } finally {
    if (run === requestId) loading.value = false;
  }
}
watch(
  () => [filter.q, filter.school, filter.major, filter.year, filter.readiness],
  () => {
    filter.page = 1;
    clearTimeout(timer);
    timer = setTimeout(refresh, 220);
  },
);
watch(() => filter.page, refresh);
async function create() {
  try {
    const b = await api.createBook({ title: "未命名图书", stock: 0 });
    await refresh();
    emit("open-book", b.id);
  } catch (e) {
    error.value = e.message;
  }
}
function batch(kind) {
  emit("batch", kind, [...selected.value]);
}
function showSchools() {
  view.value = "schools";
  schoolView.value?.refresh();
}
defineExpose({ refresh, showSchools });
onMounted(refresh);
onBeforeUnmount(() => {
  clearTimeout(timer);
  requestId++;
});
const money = (v) => (v == null ? "—" : `¥${Number(v).toFixed(2)}`);
</script>
<template>
  <section>
    <div class="module-bar">
      <nav class="segment" aria-label="图书视角">
        <button :class="{ active: view === 'books' }" @click="view = 'books'">
          按图书</button
        ><button
          :class="{ active: view === 'schools' }"
          @click="view = 'schools'"
        >
          按学校
        </button>
      </nav>
      <button class="btn primary" @click="create">
        <Plus :size="16" />新增图书
      </button>
    </div>
    <SchoolBooks
      v-show="view === 'schools'"
      ref="schoolView"
      @open-book="$emit('open-book', $event)"
      @collect="$emit('collect')"
      @updated="refresh"
    />
    <div v-show="view === 'books'">
      <div class="filter-bar">
        <label class="search-field"
          ><Search :size="16" /><input
            v-model="filter.q"
            aria-label="搜索图书"
            placeholder="书名 / ISBN / 作者" /></label
        ><SearchSelect v-model="filter.school" class="input" aria-label="学校"
          ><option value="">全部学校</option>
          <option v-for="s in data.schools" :key="s.name">
            {{ s.name }}
          </option></SearchSelect
        ><input
          v-model="filter.major"
          class="input"
          aria-label="专业"
          placeholder="专业"
        /><SearchSelect v-model="filter.year" class="input" aria-label="年份"
          ><option value="">全部年份</option>
          <option v-for="y in data.years" :key="y.year" :value="y.year">
            {{ y.year }}
          </option></SearchSelect
        ><SearchSelect
          v-model="filter.readiness"
          class="input"
          aria-label="准备情况"
          ><option value="">全部图书</option>
          <option value="version">待确认版本</option>
          <option value="sources">缺货源</option>
          <option value="images">缺图片</option></SearchSelect
        >
      </div>
      <div v-if="selected.size" class="selection-bar">
        <span>已选 {{ selected.size }} 本</span
        ><button class="btn small" @click="batch('sources')">找货源</button
        ><button class="btn small" @click="batch('images')">补图片</button
        ><button class="btn small" @click="batch('publish')">准备发布</button
        ><button class="link-btn" @click="selected.clear()">取消选择</button>
      </div>
      <p v-if="error" role="alert" class="error-panel">
        {{ error }} <button class="link-btn" @click="refresh">重试</button>
      </p>
      <div class="data-surface" :aria-busy="loading">
        <table class="data-table">
          <thead>
            <tr>
              <th class="check-col">
                <input
                  type="checkbox"
                  aria-label="全选当前页"
                  :checked="allSelected"
                  @change="toggleAll"
                />
              </th>
              <th>图书</th>
              <th>适用学校 / 专业</th>
              <th>成本 / 售价</th>
              <th>准备情况</th>
              <th>渠道</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="b in data.items" :key="b.id">
              <td>
                <input
                  type="checkbox"
                  :aria-label="'选择 ' + b.title"
                  :checked="selected.has(b.id)"
                  @change="toggle(b.id)"
                />
              </td>
              <td>
                <button class="book-cell" @click="$emit('open-book', b.id)">
                  <img
                    v-if="b.coverUrl"
                    :src="assetUrl(b.coverUrl)"
                    alt=""
                  /><span v-else class="cover-placeholder">暂无封面</span
                  ><span
                    ><strong>{{ b.title }}</strong
                    ><small
                      >{{ b.author || "作者待补充" }} ·
                      {{ b.edition || "版次待确认" }}</small
                    ><small>{{ b.isbn || "ISBN 待补充" }}</small></span
                  >
                </button>
              </td>
              <td>
                <details v-if="b.requirements?.length" class="usage-summary">
                  <summary>
                    {{ new Set(b.requirements.map((r) => r.school)).size }}
                    所学校 · {{ b.requirements.length }} 条用书
                  </summary>
                  <p v-for="(r, i) in b.requirements" :key="i">
                    {{ r.school }} · {{ r.major }} · {{ r.year }}
                  </p>
                </details>
                <span v-else class="muted">—</span>
              </td>
              <td class="numeric">
                {{ money(b.costPrice) }}<small>{{ money(b.sellPrice) }}</small>
              </td>
              <td>
                <div class="status-stack">
                  <span
                    class="tag"
                    :class="b.physicalReady ? 'success' : 'warning'"
                    >{{ b.physicalReady ? "货源可用" : "缺货源" }}</span
                  ><span
                    class="tag"
                    :class="b.imageCount ? 'neutral' : 'warning'"
                    >{{
                      b.imageCount ? b.imageCount + " 张图片" : "缺图片"
                    }}</span
                  >
                </div>
              </td>
              <td>{{ b.channelCount || 0 }}</td>
              <td>
                <button class="btn small" @click="$emit('publish', b)">
                  发布
                </button>
              </td>
            </tr>
            <tr v-if="!data.items.length">
              <td colspan="7" class="table-empty">
                {{ loading ? "正在加载…" : "暂无图书" }}
                <button
                  v-if="!loading"
                  class="link-btn"
                  @click="view = 'schools'"
                >
                  从学校用书获取
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <footer class="pagination">
        <span>{{ data.total }} 本图书</span>
        <div class="row">
          <button
            class="btn small"
            :disabled="filter.page <= 1 || loading"
            @click="filter.page--"
          >
            上一页</button
          ><span>{{ filter.page }} / {{ pages }}</span
          ><button
            class="btn small"
            :disabled="filter.page >= pages || loading"
            @click="filter.page++"
          >
            下一页
          </button>
        </div>
      </footer>
    </div>
  </section>
</template>
