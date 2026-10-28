<script setup>
import { computed, onMounted, ref } from 'vue';
import { api } from '../api';

const emit = defineEmits(['open-book', 'navigate', 'updated']);

const busy = ref(false);
const error = ref('');
const status = ref(null);
const pathInput = ref('');
const mode = ref('school');

// 采集（P1）：宿主用本机 node 运行外部 .mjs 插件，无需重新打包。
const view = ref('browse');
const adapters = ref([]);
const adaptersLoading = ref(false);
const selectedKey = ref('');
const fields = ref({});
const options = ref({ year: '2027', limit: '', delay: '', refresh: false, rebuild: true });
const collecting = ref(false);
const collectError = ref('');
const collectLog = ref(null);
const uploadOpen = ref(false);
const uploadName = ref('');
const uploadContent = ref('');

const tree = ref([]);
const selectedSchool = ref(null);
const selectedProgram = ref(null);
const majorData = ref(null);

const bookList = ref([]);
const bookQuery = ref('');
const selectedBook = ref(null);
const bookDetail = ref(null);

const stats = computed(() => status.value?.stats || null);
const ready = computed(
  () => status.value?.configured && status.value?.exists && !status.value?.error,
);

async function loadStatus() {
  busy.value = true;
  error.value = '';
  try {
    status.value = await api.commerce('/catalog/status');
    pathInput.value = status.value.override || status.value.path || '';
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}

async function loadTree() {
  const data = await api.commerce('/catalog/tree');
  tree.value = data.schools || [];
}

async function loadBooks() {
  const data = await api.commerce('/catalog/books');
  bookList.value = data.books || [];
}

function resetData() {
  tree.value = [];
  bookList.value = [];
  selectedSchool.value = null;
  selectedProgram.value = null;
  majorData.value = null;
  selectedBook.value = null;
  bookDetail.value = null;
}

async function loadActive() {
  if (!ready.value) return;
  busy.value = true;
  error.value = '';
  try {
    if (mode.value === 'school' && !tree.value.length) await loadTree();
    if (mode.value === 'book' && !bookList.value.length) await loadBooks();
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}

async function refresh() {
  resetData();
  await loadStatus();
  await loadActive();
}

async function savePath() {
  busy.value = true;
  error.value = '';
  try {
    status.value = await api.commerce('/catalog/path', { path: pathInput.value.trim() });
    resetData();
    await loadActive();
    emit('updated');
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}

function switchMode(next) {
  if (mode.value === next) return;
  mode.value = next;
  selectedBook.value = null;
  bookDetail.value = null;
  selectedProgram.value = null;
  majorData.value = null;
  loadActive();
}

function selectSchool(school) {
  selectedSchool.value = school;
  selectedProgram.value = null;
  majorData.value = null;
}

async function selectProgram(program) {
  selectedProgram.value = program;
  majorData.value = null;
  busy.value = true;
  error.value = '';
  try {
    majorData.value = await api.commerce(
      '/catalog/major/' + encodeURIComponent(program.id) + '/books',
    );
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}

const filteredBooks = computed(() => {
  const q = bookQuery.value.trim().toLowerCase();
  if (!q) return bookList.value;
  return bookList.value.filter((b) =>
    [b.title, b.authors, b.publisher, ...(b.schools || [])]
      .filter(Boolean)
      .join(' ')
      .toLowerCase()
      .includes(q),
  );
});

async function selectBook(book) {
  selectedBook.value = book;
  bookDetail.value = null;
  busy.value = true;
  error.value = '';
  try {
    bookDetail.value = await api.commerce(
      '/catalog/book/' + encodeURIComponent(book.id) + '/usage',
    );
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}

const groupedUsage = computed(() => {
  const usage = bookDetail.value?.usage || [];
  const bySchool = new Map();
  for (const u of usage) {
    if (!bySchool.has(u.school)) bySchool.set(u.school, new Map());
    const majors = bySchool.get(u.school);
    const label = [u.college, u.majorName].filter(Boolean).join(' / ') || '未分专业';
    if (!majors.has(label)) majors.set(label, []);
    majors.get(label).push(u);
  }
  return [...bySchool].map(([school, majors]) => ({
    school,
    majors: [...majors].map(([label, items]) => ({ label, items })),
  }));
});

function pillsOf(st) {
  if (!st) return [];
  const pills = [];
  if (st.listed) pills.push({ label: '已上架', tone: 'on' });
  if (st.physical) pills.push({ label: '实体货源', tone: 'on' });
  if (st.digital) pills.push({ label: '电子资源', tone: 'on' });
  if (st.confirmed > 0) pills.push({ label: `已关联 ${st.confirmed}`, tone: 'ok' });
  else if (st.imported > 0) pills.push({ label: `待确认 ${st.imported}`, tone: 'warn' });
  else pills.push({ label: '未导入', tone: 'muted' });
  return pills;
}

function linkedBookId(item) {
  return (item?.status?.bookIds || [])[0] || null;
}

function openLinked(item) {
  const id = linkedBookId(item);
  if (id) emit('open-book', id);
}

function shortTime(value) {
  if (!value) return '未知';
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? value : d.toLocaleString('zh-CN', { hour12: false });
}

const selectedAdapter = computed(
  () => adapters.value.find((a) => a.key === selectedKey.value) || null,
);

function pickAdapter(key) {
  selectedKey.value = key;
  const next = {};
  for (const field of adapters.value.find((a) => a.key === key)?.fields || []) {
    if (field.default != null) next[field.name] = field.default;
  }
  fields.value = next;
  collectLog.value = null;
}

async function loadAdapters() {
  adaptersLoading.value = true;
  collectError.value = '';
  try {
    const data = await api.commerce('/catalog/adapters');
    adapters.value = (data.adapters || []).filter((a) => !a.error);
    const broken = (data.adapters || []).filter((a) => a.error);
    if (broken.length) collectError.value = `以下插件加载失败：${broken.map((b) => b.key).join('、')}`;
    if (!adapters.value.some((a) => a.key === selectedKey.value)) {
      pickAdapter(adapters.value[0]?.key || '');
    }
  } catch (e) {
    collectError.value = e.message;
  } finally {
    adaptersLoading.value = false;
  }
}

function openView(next) {
  view.value = next;
  if (next === 'collect' && !adapters.value.length) loadAdapters();
}

function fillFromUpload(file) {
  if (!file) return;
  const reader = new FileReader();
  reader.onload = () => {
    uploadName.value = file.name;
    uploadContent.value = String(reader.result || '');
    uploadOpen.value = true;
  };
  reader.readAsText(file);
}

async function uploadAdapter() {
  const name = uploadName.value.trim();
  if (!name) return (collectError.value = '请填写插件文件名（以 .mjs 结尾）');
  if (!uploadContent.value.trim()) return (collectError.value = '插件内容为空');
  collecting.value = true;
  collectError.value = '';
  try {
    const data = await api.commerce('/catalog/adapters', { name, content: uploadContent.value });
    adapters.value = (data.adapters || []).filter((a) => !a.error);
    uploadOpen.value = false;
    const stem = name.replace(/\.mjs$/i, '');
    const added = adapters.value.find((a) => a.key === stem);
    if (added) pickAdapter(added.key);
  } catch (e) {
    collectError.value = e.message;
  } finally {
    collecting.value = false;
  }
}

async function removeAdapter(adapter) {
  collectError.value = '';
  try {
    const name = String(adapter.ref).split(/[\\/]/).pop();
    const data = await api.commerce('/catalog/delete-adapter', { name });
    adapters.value = (data.adapters || []).filter((a) => !a.error);
    if (selectedKey.value === adapter.key) pickAdapter(adapters.value[0]?.key || '');
  } catch (e) {
    collectError.value = e.message;
  }
}

async function runCollect() {
  const adapter = selectedAdapter.value;
  if (!adapter) return (collectError.value = '请选择采集插件');
  for (const field of adapter.fields || []) {
    if (field.required && !String(fields.value[field.name] || '').trim()) {
      return (collectError.value = `请填写「${field.label || field.name}」`);
    }
  }
  collecting.value = true;
  collectError.value = '';
  collectLog.value = null;
  try {
    const body = {
      plugin: adapter.ref,
      rebuild: !!options.value.rebuild,
      refresh: !!options.value.refresh,
      year: options.value.year || undefined,
      limit: options.value.limit === '' ? undefined : options.value.limit,
      delay: options.value.delay === '' ? undefined : options.value.delay,
      args: fields.value,
    };
    collectLog.value = await api.commerce('/catalog/collect', body);
    emit('updated');
    if (collectLog.value?.rebuild) await loadStatus();
  } catch (e) {
    collectError.value = e.message;
  } finally {
    collecting.value = false;
  }
}

onMounted(async () => {
  await loadStatus();
  await loadActive();
});

defineExpose({ refresh });
</script>

<template>
  <section class="panel operations-panel">
    <header class="operations-head">
      <div>
        <h2>考研目录</h2>
        <p class="hint">
          学校维度：学校 → 专业 → 参考书目。图书维度：教材 → 引用它的学校与专业，并对照项目已有内容。
        </p>
      </div>
      <div class="head-actions">
        <UiButton class="btn tiny" :class="{ accent: view === 'browse' }" @click="openView('browse')">
          浏览
        </UiButton>
        <UiButton
          class="btn tiny"
          :class="{ accent: view === 'collect' }"
          @click="openView('collect')"
        >
          采集
        </UiButton>
        <UiButton class="btn ghost" :disabled="busy" @click="refresh">刷新</UiButton>
      </div>
    </header>

    <p v-if="error" class="xy-error" role="alert">{{ error }}</p>

    <!-- 采集面板：选择/上传插件 → 填写参数 → 运行 → 重建目录 -->
    <div v-if="view === 'collect'" class="cat-collect">
      <p v-if="collectError" class="xy-error" role="alert">{{ collectError }}</p>

      <div class="business-card">
        <div class="cat-field cat-field--row">
          <label for="cat-plugin">采集插件</label>
          <div class="cat-plugin-row">
            <select
              id="cat-plugin"
              class="input"
              :value="selectedKey"
              :disabled="adaptersLoading"
              @change="pickAdapter($event.target.value)"
            >
              <option v-for="a in adapters" :key="a.key" :value="a.key">
                {{ a.name || a.key }}{{ a.builtin ? '' : ' · 自定义' }}
              </option>
            </select>
            <UiButton class="btn tiny ghost" :disabled="adaptersLoading" @click="loadAdapters">
              重新加载
            </UiButton>
          </div>
          <p v-if="selectedAdapter?.description" class="hint">{{ selectedAdapter.description }}</p>
          <p v-if="adaptersLoading" class="hint">正在读取插件…</p>
          <p v-else-if="!adapters.length" class="hint">
            还没有可用插件。请确认仓库 <code>scripts/kaoyan/adapters</code> 存在，或拖入自定义
            <code>.mjs</code>。
          </p>
        </div>

        <div v-if="selectedAdapter" class="cat-fields">
          <div
            v-for="field in selectedAdapter.fields || []"
            :key="field.name"
            class="cat-field cat-field--row"
          >
            <label :for="'cat-f-' + field.name">
              {{ field.label || field.name }}<em v-if="field.required" class="cat-req">*</em>
            </label>
            <input
              :id="'cat-f-' + field.name"
              v-model="fields[field.name]"
              class="input"
              :placeholder="field.placeholder || ''"
            />
          </div>

          <div class="cat-field cat-field--row">
            <label for="cat-year">年份</label>
            <input id="cat-year" v-model="options.year" class="input" placeholder="2027" />
          </div>
          <div class="cat-field cat-field--row">
            <label for="cat-limit">仅采前 N 条（留空=全部）</label>
            <input
              id="cat-limit"
              v-model="options.limit"
              class="input"
              inputmode="numeric"
              placeholder="如 5（调试用）"
            />
          </div>
          <div class="cat-field cat-field--row">
            <label for="cat-delay">请求间隔 ms</label>
            <input
              id="cat-delay"
              v-model="options.delay"
              class="input"
              inputmode="numeric"
              placeholder="800"
            />
          </div>

          <div class="cat-checks">
            <label class="cat-check">
              <input v-model="options.refresh" type="checkbox" /> 忽略缓存，强制重新抓取
            </label>
            <label class="cat-check">
              <input v-model="options.rebuild" type="checkbox" /> 采集后重建 catalog.db
            </label>
          </div>

          <div class="head-actions">
            <UiButton class="btn" :disabled="collecting || !selectedAdapter" @click="runCollect">
              {{ collecting ? '采集中…' : '运行采集' }}
            </UiButton>
          </div>
          <p v-if="collecting" class="hint">
            正在用本机 node 运行插件，联网抓取会较慢，请稍候…
          </p>
        </div>
      </div>

      <div
        class="business-card cat-upload"
        @dragover.prevent
        @drop.prevent="fillFromUpload($event.dataTransfer?.files?.[0])"
      >
        <div class="business-row">
          <strong>自定义插件</strong>
          <span class="hint">拖入 <code>.mjs</code>，或选择本地文件上传（存于 data/kaoyan/adapters）</span>
        </div>
        <div class="head-actions">
          <label class="btn tiny ghost cat-file">
            选择文件
            <input
              type="file"
              accept=".mjs,text/javascript"
              @change="fillFromUpload($event.target.files?.[0])"
            />
          </label>
        </div>

        <template v-if="uploadOpen">
          <div class="cat-field cat-field--row">
            <label for="cat-up-name">文件名</label>
            <input id="cat-up-name" v-model="uploadName" class="input" placeholder="my-school.mjs" />
          </div>
          <textarea
            v-model="uploadContent"
            class="input cat-textarea"
            rows="8"
            spellcheck="false"
            placeholder="export default { meta: { key, name, fields: [...] }, async run({ host, args }) { return { payload } } }"
          ></textarea>
          <div class="head-actions">
            <UiButton class="btn tiny" :disabled="collecting" @click="uploadAdapter">保存插件</UiButton>
            <UiButton class="btn tiny ghost" @click="uploadOpen = false">取消</UiButton>
          </div>
        </template>

        <div v-if="adapters.some((a) => !a.builtin)" class="cat-userlist">
          <p v-for="a in adapters.filter((x) => !x.builtin)" :key="a.key" class="cat-userrow">
            <code>{{ a.name || a.key }}</code>
            <UiButton class="btn tiny ghost" :disabled="collecting" @click="removeAdapter(a)">
              删除
            </UiButton>
          </p>
        </div>
      </div>

      <div v-if="collectLog" class="business-card cat-result">
        <h4 class="cat-college">采集结果</h4>
        <p class="hint">
          写入 <code>{{ collectLog.collect?.file }}</code> ·
          {{ collectLog.collect?.programs ?? 0 }} 专业 / {{ collectLog.collect?.books ?? 0 }} 本教材
          <template v-if="collectLog.rebuild">
            · 已重建目录：{{ collectLog.rebuild.schools }} 校 / {{ collectLog.rebuild.books }} 本
          </template>
        </p>
        <details>
          <summary>原始输出</summary>
          <pre class="cat-pre">{{ JSON.stringify(collectLog, null, 2) }}</pre>
        </details>
      </div>
    </div>

    <template v-else>
    <div v-if="!ready" class="business-notice">
      <p><strong>尚未连接目录数据库</strong> <code>catalog.db</code>。</p>
      <p class="hint">
        先在仓库根目录运行 <code>npm run kaoyan:catalog</code> 生成目录，或在下方粘贴
        <code>catalog.db</code> 的绝对路径。目录以只读方式打开，不会被修改。
      </p>
      <p v-if="status" class="hint">
        已尝试：<code>{{ status.path || '（无）' }}</code> · 数据目录候选：<code>{{
          status.dataDirCandidate
        }}</code>
        <template v-if="status.error"> · {{ status.error }}</template>
      </p>
      <form class="cat-pathform" @submit.prevent="savePath">
        <input
          v-model="pathInput"
          class="input"
          placeholder="例如 E:\workspace\git\kong\data\kaoyan\catalog\catalog.db"
          aria-label="catalog.db 路径"
        />
        <UiButton class="btn" :disabled="busy">保存路径</UiButton>
      </form>
    </div>

    <template v-else>
      <div class="business-notice cat-status">
        <span>
          目录已连接 · {{ stats?.schools ?? 0 }} 校 / {{ stats?.programs ?? 0 }} 专业 /
          {{ stats?.books ?? 0 }} 本教材 · 采集于 {{ shortTime(stats?.latestCollectedAt) }}
        </span>
        <details>
          <summary>路径</summary>
          <form class="cat-pathform" @submit.prevent="savePath">
            <input v-model="pathInput" class="input" aria-label="catalog.db 路径" />
            <UiButton class="btn tiny" :disabled="busy">保存</UiButton>
          </form>
          <p class="hint">当前：<code>{{ status.path }}</code></p>
        </details>
      </div>

      <div class="business-tabs">
        <UiButton
          class="btn tiny"
          :class="{ accent: mode === 'school' }"
          @click="switchMode('school')"
        >
          学校维度
        </UiButton>
        <UiButton class="btn tiny" :class="{ accent: mode === 'book' }" @click="switchMode('book')">
          图书维度
        </UiButton>
      </div>

      <!-- 学校维度 -->
      <template v-if="mode === 'school'">
        <div class="business-columns">
          <div class="business-scroll">
            <UiButton
              v-for="school in tree"
              :key="school.id"
              class="business-task"
              :class="{ 'cat-active': selectedSchool?.id === school.id }"
              @click="selectSchool(school)"
            >
              <div>
                <strong>{{ school.name }}</strong>
                <p>{{ school.programCount }} 专业 · {{ school.bookCount }} 本教材</p>
              </div>
              <span>→</span>
            </UiButton>
            <p v-if="!tree.length" class="empty">目录中没有学校</p>
          </div>

          <div class="business-scroll">
            <template v-if="selectedSchool">
              <div v-for="(college, i) in selectedSchool.colleges" :key="i" class="business-card">
                <h4 class="cat-college">{{ college.college || '未分学院' }}</h4>
                <UiButton
                  v-for="program in college.programs"
                  :key="program.id"
                  class="business-task"
                  :class="{ 'cat-active': selectedProgram?.id === program.id }"
                  @click="selectProgram(program)"
                >
                  <div>
                    <strong>{{ program.majorName || '（未命名专业）' }}</strong>
                    <p>
                      {{ program.majorCode || '无代码' }} · {{ program.year }} ·
                      {{ program.bookCount }} 本
                    </p>
                  </div>
                  <span>→</span>
                </UiButton>
              </div>
            </template>
            <p v-else class="empty">选择左侧学校，展开其学院与专业</p>
          </div>
        </div>

        <div v-if="selectedProgram" class="business-card cat-major">
          <div class="business-row">
            <strong>
              {{ majorData?.program?.school || selectedSchool?.name }} ·
              {{ majorData?.program?.college || '未分学院' }} /
              {{ majorData?.program?.majorName || selectedProgram.majorName }}
            </strong>
            <a
              v-if="/^https?:\/\//.test(majorData?.program?.pageUrl || '')"
              :href="majorData.program.pageUrl"
              target="_blank"
              rel="noreferrer"
              >来源页面</a
            >
          </div>
          <p v-if="busy && !majorData" class="hint">正在加载该专业的参考书目…</p>
          <template v-for="subject in majorData?.subjects || []" :key="subject.id">
            <h4 class="cat-subject">
              {{ subject.stage }} · {{ subject.code }} {{ subject.name }}
            </h4>
            <article v-for="book in subject.books" :key="subject.id + book.id" class="cat-book">
              <div class="business-row">
                <strong>{{ book.title }}</strong>
                <span class="cat-pills">
                  <i v-for="(p, k) in pillsOf(book.status)" :key="k" :class="['cat-pill', p.tone]">{{
                    p.label
                  }}</i>
                </span>
              </div>
              <p class="hint">
                {{ [book.authors, book.publisher, book.edition, book.year].filter(Boolean).join(' · ') ||
                '版本信息待补' }}
              </p>
              <p v-if="book.raw" class="cat-raw">{{ book.raw }}</p>
              <div class="head-actions">
                <a
                  v-if="/^https?:\/\//.test(book.sourceUrl || '')"
                  :href="book.sourceUrl"
                  target="_blank"
                  rel="noreferrer"
                  >采集来源</a
                >
                <UiButton v-if="linkedBookId(book)" class="btn tiny" @click="openLinked(book)">
                  打开标准书目
                </UiButton>
                <UiButton
                  v-else-if="book.status?.imported > 0"
                  class="btn tiny ghost"
                  @click="emit('navigate', 'schools')"
                >
                  去学校书单确认
                </UiButton>
                <span v-else class="hint">尚未导入，请先在「学校书单」导入 catalog.json</span>
              </div>
            </article>
            <p v-if="!subject.books?.length" class="hint cat-none">该科目未采集到参考书目</p>
          </template>
          <p v-if="majorData && !majorData.subjects?.length" class="empty">
            该专业暂无科目或参考书目
          </p>
        </div>
      </template>

      <!-- 图书维度 -->
      <template v-else>
        <div class="business-filters">
          <input
            v-model="bookQuery"
            class="input"
            placeholder="搜索书名 / 作者 / 出版社 / 学校"
            aria-label="搜索教材"
          />
        </div>
        <div class="business-columns">
          <div class="business-scroll">
            <UiButton
              v-for="book in filteredBooks"
              :key="book.id"
              class="business-task"
              :class="{ 'cat-active': selectedBook?.id === book.id }"
              @click="selectBook(book)"
            >
              <div>
                <strong>{{ book.title }}</strong>
                <p>{{ [book.authors, book.publisher].filter(Boolean).join(' · ') || '暂无作者/出版社' }}</p>
                <small
                  >{{ book.schoolCount }} 校 · {{ book.programCount }} 专业 ·
                  {{ book.requirementCount }} 次引用</small
                >
              </div>
              <span class="cat-pills">
                <i v-for="(p, k) in pillsOf(book.status)" :key="k" :class="['cat-pill', p.tone]">{{
                  p.label
                }}</i>
              </span>
            </UiButton>
            <p v-if="!filteredBooks.length" class="empty">没有匹配的教材</p>
          </div>

          <div class="business-scroll">
            <template v-if="selectedBook">
              <div class="business-card">
                <div class="business-row">
                  <strong>{{ selectedBook.title }}</strong>
                  <span class="cat-pills">
                    <i
                      v-for="(p, k) in pillsOf(bookDetail?.book?.status || selectedBook.status)"
                      :key="k"
                      :class="['cat-pill', p.tone]"
                      >{{ p.label }}</i
                    >
                  </span>
                </div>
                <p class="hint">
                  {{
                    [
                      bookDetail?.book?.authors,
                      bookDetail?.book?.publisher,
                      bookDetail?.book?.edition,
                      bookDetail?.book?.year,
                    ]
                      .filter(Boolean)
                      .join(' · ') || '版本信息待补'
                  }}
                </p>
                <div class="head-actions">
                  <UiButton
                    v-if="linkedBookId(bookDetail?.book || selectedBook)"
                    class="btn tiny"
                    @click="openLinked(bookDetail?.book || selectedBook)"
                  >
                    打开标准书目
                  </UiButton>
                  <UiButton
                    v-else
                    class="btn tiny ghost"
                    @click="emit('navigate', 'schools')"
                  >
                    去学校书单导入 / 确认
                  </UiButton>
                </div>
              </div>

              <p v-if="busy && !bookDetail" class="hint">正在加载引用清单…</p>
              <div v-for="group in groupedUsage" :key="group.school" class="business-card">
                <h4 class="cat-college">{{ group.school }}</h4>
                <div v-for="major in group.majors" :key="major.label" class="cat-usage">
                  <strong>{{ major.label }}</strong>
                  <p v-for="(u, i) in major.items" :key="i" class="hint">
                    {{ u.year }} · {{ u.stage }} {{ u.subjectCode }} {{ u.subjectName }}
                    <a
                      v-if="/^https?:\/\//.test(u.sourceUrl || '')"
                      :href="u.sourceUrl"
                      target="_blank"
                      rel="noreferrer"
                      >来源</a
                    >
                  </p>
                </div>
              </div>
              <p v-if="bookDetail && !groupedUsage.length" class="empty">没有引用记录</p>
            </template>
            <p v-else class="empty">选择左侧教材，查看引用它的学校与专业</p>
          </div>
        </div>
      </template>
    </template>
    </template>
  </section>
</template>

<style scoped>
.cat-collect {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.cat-fields {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px solid var(--line);
}
.cat-field--row {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.cat-field label {
  font-size: 12px;
  color: var(--muted);
  font-weight: 600;
}
.cat-req {
  color: #b4541f;
  font-style: normal;
  margin-left: 2px;
}
.cat-plugin-row {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.cat-plugin-row .input {
  flex: 1 1 240px;
  min-width: 0;
}
.cat-checks {
  display: flex;
  gap: 18px;
  flex-wrap: wrap;
}
.cat-check {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--ink);
}
.cat-upload {
  border-style: dashed;
}
.cat-file {
  display: inline-flex;
  align-items: center;
  cursor: pointer;
}
.cat-file input {
  display: none;
}
.cat-textarea {
  width: 100%;
  font-family: ui-monospace, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.5;
  resize: vertical;
  margin-top: 6px;
}
.cat-userlist {
  margin-top: 12px;
  border-top: 1px solid var(--line);
  padding-top: 10px;
}
.cat-userrow {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  margin: 6px 0;
}
.cat-result {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.cat-pre {
  background: var(--paper-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 10px;
  font-size: 12px;
  line-height: 1.5;
  overflow: auto;
  max-height: 320px;
  white-space: pre-wrap;
  word-break: break-all;
}
.cat-pathform {
  display: flex;
  gap: 10px;
  margin-top: 10px;
  flex-wrap: wrap;
}
.cat-pathform .input {
  flex: 1 1 320px;
  min-width: 0;
}
.cat-status {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
}
.cat-status details summary {
  cursor: pointer;
  font-size: 12px;
}
.cat-college {
  margin: 0 0 6px;
  font-size: 13px;
  color: var(--muted);
  font-weight: 600;
}
.cat-subject {
  margin: 16px 0 6px;
  font-size: 13px;
  color: var(--focus);
  font-weight: 600;
}
.cat-major {
  margin-top: 18px;
}
.cat-book {
  border-top: 1px solid var(--line);
  padding: 12px 0;
}
.cat-book:first-of-type {
  border-top: 0;
}
.cat-book p {
  margin: 6px 0;
}
.cat-raw {
  font-size: 12px;
  color: var(--muted);
  overflow-wrap: anywhere;
}
.cat-none {
  padding: 6px 0 12px;
}
.cat-usage {
  border-top: 1px solid var(--line);
  padding: 8px 0;
}
.cat-usage:first-of-type {
  border-top: 0;
}
.cat-usage p {
  margin: 4px 0;
}
.cat-pills {
  display: inline-flex;
  gap: 5px;
  flex-wrap: wrap;
  justify-content: flex-end;
}
.cat-pill {
  font-style: normal;
  font-size: 11px;
  line-height: 1;
  padding: 4px 8px;
  border-radius: 999px;
  border: 1px solid var(--line-strong);
  color: var(--muted);
  white-space: nowrap;
}
.cat-pill.on {
  background: #e7efe2;
  border-color: #c3d6bb;
  color: #3f6b3a;
}
.cat-pill.ok {
  background: #eaf0f5;
  border-color: #c5d5e2;
  color: #3d5f7a;
}
.cat-pill.warn {
  background: #f6f0e6;
  border-color: #e4d9c7;
  color: #705633;
}
.cat-pill.muted {
  background: var(--paper);
}
.business-task.cat-active {
  background: var(--paper-2);
  box-shadow: inset 2px 0 0 var(--focus);
}
code {
  overflow-wrap: anywhere;
}
</style>
