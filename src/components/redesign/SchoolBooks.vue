<script setup>
import { computed, onMounted, onBeforeUnmount, reactive, ref, watch } from "vue";
import { api, openBrowser } from "../../api";
const emit = defineEmits(["open-book", "collect", "updated"]);
const inventory = ref([]);
const records = ref([]),
  loading = ref(false),
  error = ref(""),
  school = ref(""),
  program = ref(""),
  year = ref(""),
  query = ref("");
const matching = ref(null),
  candidates = ref([]),
  search = ref(""),
  busy = ref(false);
const form = reactive({ bookId: "", editionChecked: false, book: {} });
const online = ref(null), onlineQuery = ref(""), onlineError = ref("");
let onlineTimer, onlineDeadline = 0;
function closeMatch() { matching.value = null; clearInterval(onlineTimer); }
function refreshOnline() {
  onlineDeadline = Date.now() + 120000;
  onlineError.value = "";
  return pollOnline();
}
async function pollOnline() {
  if (document.hidden) return;
  if (Date.now() > onlineDeadline) {
    clearInterval(onlineTimer);
    onlineError.value = '检索等待超时，请检查网站窗口后重新检索。';
    return;
  }
  const id = matching.value?.id;
  if (!id) return;
  try {
    const result = await api.commerce(`/requirements/${id}/search`);
    if (matching.value?.id !== id) return;
    online.value = result;
    if (result.state === 'completed') clearInterval(onlineTimer);
  } catch (e) { onlineError.value = e.message; }
}
async function searchOnline() {
  clearInterval(onlineTimer);
  onlineError.value = "";
  const id = matching.value?.id;
  if (!id) return;
  try {
    const result = await api.commerce(`/requirements/${id}/search`, { action: 'start', query: onlineQuery.value });
    if (matching.value?.id !== id) return;
    online.value = result;
    onlineQuery.value = result.query;
    await openBrowser('kongfz', result.searchUrl);
    onlineDeadline = Date.now() + 120000;
    onlineTimer = setInterval(pollOnline, 2000);
  } catch (e) { onlineError.value = e.message; }
}
async function useOnline(b) {
  form.bookId = "";
  Object.assign(form.book, b);
  form.editionChecked = false;
  form.lookupEvidence = { ...b, query: online.value.query, retrievedAt: online.value.updatedAt };
  search.value = b.isbn || b.title;
  await lookup();
  const existing = b.isbn && candidates.value.find(c => c.isbn === b.isbn);
  if (existing) form.bookId = existing.id;
}
onBeforeUnmount(() => clearInterval(onlineTimer));
const schools = computed(() =>
  [
    ...new Set([
      ...records.value.map((r) => r.data.school),
      ...inventory.value.map((p) => p.school),
    ]),
  ]
    .filter((s) => s.includes(query.value))
    .sort(),
);
const years = computed(() =>
  [
    ...new Set(
      [...records.value.map((r) => r.data), ...inventory.value]
        .filter((r) => r.school === school.value)
        .map((r) => r.year),
    ),
  ].sort((a, b) => b - a),
);
const scoped = computed(() =>
  records.value.filter(
    (r) =>
      r.data.school === school.value &&
      (!year.value || r.data.year === Number(year.value)),
  ),
);
const key = (r) =>
  [
    r.data.college || "",
    r.data.majorCode || "",
    r.data.major || "未注明专业",
  ].join("|");
const programs = computed(() => {
  const m = new Map();
  for (const d of inventory.value.filter(
    (d) =>
      d.school === school.value &&
      (!year.value || d.year === Number(year.value)),
  )) {
    const k = key({ data: d });
    m.set(k, {
      key: k,
      college: d.college,
      name: d.major || "未注明专业",
      code: d.majorCode,
      count: 0,
    });
  }
  for (const r of scoped.value) {
    const k = key(r);
    if (!m.has(k))
      m.set(k, {
        key: k,
        college: r.data.college,
        name: r.data.major || "未注明专业",
        code: r.data.majorCode,
        count: 0,
      });
    m.get(k).count++;
  }
  return [...m.values()];
});
const colleges = computed(() =>
  [...new Set(programs.value.map((p) => p.college || "未分组"))].map(
    (name) => ({
      name,
      programs: programs.value.filter((p) => (p.college || "未分组") === name),
    }),
  ),
);
const books = computed(() =>
  scoped.value.filter((r) => key(r) === program.value),
);
watch(school, () => {
  program.value = "";
  year.value = "";
});
watch(year, () => (program.value = ""));
async function refresh() {
  loading.value = true;
  error.value = "";
  try {
    await api.commerce("/catalog/sync", {});
    const r = await api.commerce("/requirements");
    records.value = r.records;
    inventory.value = r.programs || [];
  } catch (e) {
    error.value = e.message;
  } finally {
    loading.value = false;
  }
}
async function match(r) {
  clearInterval(onlineTimer);
  online.value = null;
  onlineQuery.value = "";
  onlineError.value = "";
  matching.value = r;
  const b = r.data.candidate;
  Object.assign(form, {
    bookId: "",
    editionChecked: false,
    lookupEvidence: null,
    book: {
      title: b.title || "",
      author: Array.isArray(b.authors) ? b.authors.join("、") : b.authors || "",
      publisher: b.publisher || "",
      isbn: b.isbn || "",
      edition: r.data.ambiguous ? "" : b.edition || "",
      volume: r.data.ambiguous ? "" : b.volume || "",
      publicationYear: String(b.year || ""),
    },
  });
  search.value = b.title;
  await Promise.all([lookup(), searchOnline()]);
}
let lookupRun = 0;
async function lookup() {
  const run = ++lookupRun;
  try {
    const r = await api.commerce(
      "/library?" + new URLSearchParams({ q: search.value, pageSize: 30 }),
    );
    if (run === lookupRun) candidates.value = r.items;
  } catch (e) {
    error.value = e.message;
  }
}
async function save() {
  busy.value = true;
  error.value = "";
  try {
    const r = await api.commerce(
      "/requirements/" + matching.value.id + "/confirm",
      { ...form, bookId: form.bookId ? Number(form.bookId) : null },
    );
    closeMatch();
    await refresh();
    emit("updated");
    emit("open-book", r.bookId);
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
onMounted(refresh);
defineExpose({ refresh });
</script>
<template>
  <div>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
    <div class="school-layout">
      <aside class="school-list">
        <input
          class="input"
          v-model="query"
          placeholder="搜索学校"
          aria-label="搜索学校"
        /><button
          v-for="name in schools"
          :key="name"
          :class="{ active: school === name }"
          @click="school = name"
        >
          <strong>{{ name }}</strong
          ><small
            >{{
              new Set([
                ...records.filter((r) => r.data.school === name).map(key),
                ...inventory
                  .filter((p) => p.school === name)
                  .map((p) => key({ data: p })),
              ]).size
            }}
            个专业</small
          >
        </button>
        <div v-if="!schools.length" class="empty-state">
          {{ loading ? "正在加载…" : "暂无学校"
          }}<button class="btn" @click="$emit('collect')">采集学校用书</button>
        </div>
      </aside>
      <section class="school-content">
        <div v-if="school" class="module-bar">
          <div class="row">
            <button v-if="program" class="btn small" @click="program = ''">
              返回专业
            </button>
            <h2>
              {{
                program ? programs.find((p) => p.key === program)?.name : school
              }}
            </h2>
          </div>
          <SearchSelect
            v-model="year"
            class="input narrow"
            aria-label="用书年份"
            ><option value="">全部年份</option>
            <option v-for="y in years" :key="y" :value="y">
              {{ y }}
            </option></SearchSelect
          >
        </div>
        <div v-if="!school" class="empty-state">选择学校</div>
        <table v-else-if="!program" class="data-table">
          <thead>
            <tr>
              <th>专业</th>
              <th>学院</th>
              <th>用书</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="college in colleges" :key="college.name"
              ><tr class="college-group">
                <th colspan="3">{{ college.name }}</th>
              </tr>
              <tr v-for="p in college.programs" :key="p.key">
                <td>
                  <button class="text-button" @click="program = p.key">
                    {{ p.name }}</button
                  ><small>{{ p.code }}</small>
                </td>
                <td>{{ p.college || "—" }}</td>
                <td>{{ p.count }}</td>
              </tr></template
            >
          </tbody>
        </table>
        <table v-else class="data-table">
          <thead>
            <tr>
              <th>参考书目</th>
              <th>科目 / 阶段</th>
              <th>年份</th>
              <th>状态</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="!books.length">
              <td colspan="5" class="table-empty">该专业暂无公布用书</td>
            </tr>
            <tr v-for="r in books" :key="r.id">
              <td>
                <strong>{{ r.data.candidate.title }}</strong
                ><small>{{ r.data.candidate.edition || "版次待核对" }}</small>
                <details class="source-evidence">
                  <summary>书目原文</summary>
                  <p>{{ r.data.raw }}</p>
                  <a
                    v-if="/^https?:/.test(r.data.sourceUrl)"
                    :href="r.data.sourceUrl"
                    target="_blank"
                    rel="noreferrer"
                    >官方来源</a
                  >
                </details>
              </td>
              <td>
                {{ r.data.subject }}<small>{{ r.data.stage }}</small>
              </td>
              <td>{{ r.data.year }}</td>
              <td>
                <span
                  class="tag"
                  :class="r.state === 'confirmed' ? 'success' : 'warning'"
                  >{{ r.state === "confirmed" ? "已关联" : "待匹配" }}</span
                >
              </td>
              <td>
                <button
                  class="btn small"
                  @click="
                    r.state === 'confirmed' && r.bookId
                      ? $emit('open-book', r.bookId)
                      : match(r)
                  "
                >
                  {{
                    r.state === "confirmed" && r.bookId
                      ? "查看图书"
                      : "匹配图书"
                  }}
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </section>
    </div>
    <div v-if="matching" class="modal-backdrop" @click.self="closeMatch">
      <section
        class="modal match-dialog"
        v-modal-focus="
          () => {
            closeMatch();
          }
        "
        role="dialog"
        aria-modal="true"
        aria-labelledby="match-title"
      >
        <div class="module-bar">
          <h2 id="match-title">匹配图书</h2>
          <button class="btn small" @click="closeMatch">关闭</button>
        </div>
        <p class="evidence-text">{{ matching.data.raw }}</p>
        <div class="row">
          <input v-model="onlineQuery" class="input" aria-label="在线检索词" placeholder="书名、作者、版次或 ISBN" @keydown.enter.prevent="searchOnline" />
          <button type="button" class="btn" @click="searchOnline">孔夫子检索</button>
          <button v-if="online" type="button" class="btn ghost" @click="refreshOnline">刷新结果</button>
        </div>
        <p v-if="onlineError || online?.error" class="error-panel" role="alert">{{ onlineError || online.error }}</p>
        <p v-else-if="online?.state === 'searching'" class="muted" role="status">正在网站检索；如需登录，请在检索窗口完成后点击重试书目检索。</p>
        <p v-else-if="online?.state === 'completed' && !online.items.length" class="muted">未找到候选，调整检索词重试。</p>
        <div v-if="online?.items?.length" class="data-surface" style="max-height: 240px; overflow: auto; margin: 12px 0">
          <table class="data-table">
            <thead><tr><th>网站候选</th><th>版本 / ISBN</th><th></th></tr></thead>
            <tbody><tr v-for="(b, i) in online.items" :key="i">
              <td>{{ b.title }}<small>{{ b.author }} · {{ b.publisher }}</small></td>
              <td>{{ b.edition || '版次待核对' }}<small>{{ b.isbn || 'ISBN 待补充' }}</small></td>
              <td><button class="btn small" @click="useOnline(b)">使用</button> <button class="btn ghost small" @click="openBrowser('kongfz', b.sourceUrl)">来源</button></td>
            </tr></tbody>
          </table>
        </div>
        <form @submit.prevent="save">
          <div class="row">
            <input
              v-model="search"
              class="input"
              aria-label="搜索已有图书"
              placeholder="搜索已有图书"
            /><button type="button" class="btn" @click="lookup">查本地</button>
          </div>
          <label class="field"
            ><span>关联图书</span
            ><SearchSelect v-model="form.bookId" class="input"
              ><option value="">新建图书版本</option>
              <option v-for="b in candidates" :key="b.id" :value="b.id">
                {{ b.title }} · {{ b.edition || "未注明版次" }} · {{ b.isbn }}
              </option></SearchSelect
            ></label
          >
          <div v-if="!form.bookId" class="form-grid">
            <label
              class="field"
              v-for="(label, k) in {
                title: '书名',
                author: '作者',
                publisher: '出版社',
                isbn: 'ISBN',
                edition: '版次',
                volume: '册次',
              }"
              :key="k"
              ><span>{{ label }}</span
              ><input
                v-model="form.book[k]"
                class="input"
                :required="k === 'title'"
            /></label>
          </div>
          <label class="check-line"
            ><input
              type="checkbox"
              v-model="form.editionChecked"
            />已核对版本和册次</label
          >
          <p v-if="error" class="error-panel">{{ error }}</p>
          <button class="btn primary" :disabled="busy || !form.editionChecked">
            确认关联
          </button>
        </form>
      </section>
    </div>
  </div>
</template>
