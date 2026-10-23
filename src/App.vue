<script setup>
import { computed, onBeforeUnmount, onMounted, nextTick, ref, watch } from 'vue';
import { api, initApi, openBrowser, searchUrl } from './api';
import { useToast } from './composables/toast';
import TopBar from './components/TopBar.vue';
import StatsRow from './components/StatsRow.vue';
import BookTable from './components/BookTable.vue';
import BookDrawer from './components/BookDrawer.vue';
import ImageEditor from './components/ImageEditor.vue';
import SettingsModal from './components/SettingsModal.vue';
import ToastStack from './components/ToastStack.vue';
import XianyuWorkbench from './components/XianyuWorkbench.vue';
import XianyuPublish from './components/XianyuPublish.vue';
import OperationsWorkspace from './components/OperationsWorkspace.vue';
import './commerce.css';

const toast = useToast();
const { toasts, remove } = toast;

const activeView = ref('home');
const operations = ref(null);
const schoolFilter = ref(''),
  yearFilter = ref(''),
  majorFilter = ref(''),
  resourceFilter = ref('');
const bookSchools = computed(() => [
  ...new Set(books.value.flatMap((b) => (b.requirements || []).map((r) => r.school))),
]);
const bookYears = computed(() =>
  [...new Set(books.value.flatMap((b) => (b.requirements || []).map((r) => r.year)))].sort(
    (a, b) => b - a,
  ),
);
const filteredBooks = computed(() =>
  books.value.filter(
    (b) =>
      (!query.value ||
        [b.title, b.author, b.publisher, b.isbn]
          .join(' ')
          .toLowerCase()
          .includes(query.value.toLowerCase())) &&
      (!statusFilter.value || b.status === statusFilter.value) &&
      ((!schoolFilter.value && !yearFilter.value && !majorFilter.value) ||
        (b.requirements || []).some(
          (r) =>
            (!schoolFilter.value || r.school === schoolFilter.value) &&
            (!yearFilter.value || r.year === Number(yearFilter.value)) &&
            (!majorFilter.value || r.major.includes(majorFilter.value)),
        )) &&
      (!resourceFilter.value ||
        (resourceFilter.value === 'physical' && !b.physicalReady) ||
        (resourceFilter.value === 'digital' && !b.digitalReady) ||
        (resourceFilter.value === 'complete' && b.physicalReady && b.digitalReady) ||
        (resourceFilter.value === 'listed' && b.listed)),
  ),
);
async function navigateOperations(view, orderId) {
  activeView.value = view;
  await nextTick();
  if (orderId) await operations.value?.openOrder(orderId);
}
const xianyuWorkbench = ref(null);
const xianyuPublish = ref(null);
const ready = ref(false);
const bootError = ref('');
const loading = ref(false);
const publishing = ref(false);
const stats = ref({});
const books = ref([]);
const settings = ref({});
const query = ref('');
const statusFilter = ref('');
const selected = ref(null);
const editorImage = ref(null);
const settingsOpen = ref(false);

const statusMap = new Map();
let pollTimer = null;
let searchTimer = null;

const filterLabel = computed(() => {
  if (statusFilter.value === 'draft') return '待整理';
  if (statusFilter.value === 'ready') return '待发布';
  if (statusFilter.value === 'listed') return '已上架';
  if (statusFilter.value === 'sold') return '已售出';
  return '全部状态';
});

async function bootstrap() {
  bootError.value = '';
  try {
    await initApi();
    await Promise.all([refreshBooks(), refreshStats(), loadSettings()]);
    ready.value = true;
    startPolling();
    const cfg = await api.xianyuConfig().catch(() => null);
    if (cfg?.appKey && cfg?.secretConfigured)
      api
        .commerce('/sync-orders', {})
        .then(() => operations.value?.refresh())
        .catch((e) => toast.error('启动订单同步：' + e.message));
  } catch (error) {
    bootError.value = error.message || '无法连接本地服务';
  }
}

async function refreshBooks(quiet = false) {
  if (!quiet) loading.value = true;
  try {
    books.value = await api.commerce('/books');
    detectPublished();
  } catch (error) {
    if (!quiet) toast.error(error.message);
  } finally {
    if (!quiet) loading.value = false;
  }
}

async function refreshStats() {
  try {
    stats.value = await api.stats();
  } catch {
    /* 静默 */
  }
}

async function loadSettings() {
  try {
    settings.value = await api.settings();
  } catch {
    settings.value = {};
  }
}

function detectPublished() {
  for (const book of books.value) {
    const previous = statusMap.get(book.id);
    if (previous && previous !== 'published' && book.listingStatus === 'published') {
      toast.success(`《${book.title}》已发布到闲鱼`);
    }
    statusMap.set(book.id, book.listingStatus);
  }
}

function startPolling() {
  stopPolling();
  pollTimer = setInterval(async () => {
    if (document.hidden) return;
    await Promise.all([refreshBooks(true), refreshStats()]);
  }, 8000);
}

function stopPolling() {
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = null;
}

watch(query, () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => refreshBooks(), 260);
});

watch(statusFilter, () => refreshBooks());

async function openBookOrder(orderNo) {
  selected.value = null;
  activeView.value = 'xianyu';
  await nextTick();
  await xianyuWorkbench.value?.openOrder(orderNo);
}

async function selectBook(id) {
  try {
    selected.value = await api.book(id);
  } catch (error) {
    toast.error(error.message);
  }
}

async function refreshSelected() {
  if (!selected.value) return;
  try {
    selected.value = await api.book(selected.value.id);
  } catch {
    /* 静默 */
  }
}

async function refreshSelectedImages() {
  if (!selected.value) return;
  try {
    const detail = await api.book(selected.value.id);
    selected.value.images = detail.images;
    selected.value.imageCount = detail.imageCount;
    selected.value.coverUrl = detail.coverUrl;
    selected.value.listings = detail.listings;
  } catch {
    /* 静默 */
  }
}

async function saveBook(patch) {
  try {
    const detail = await api.updateBook(selected.value.id, patch);
    selected.value = detail;
    await Promise.all([refreshBooks(true), refreshStats()]);
    toast.success('已保存');
    return detail;
  } catch (error) {
    toast.error(error.message);
    return null;
  }
}

async function createManual() {
  try {
    const detail = await api.createBook({
      title: '未命名图书',
      condition: settings.value.defaultCondition || '九成新',
    });
    await refreshBooks();
    selected.value = detail;
    toast.info('已新建一条书目，补全信息后即可找图');
  } catch (error) {
    toast.error(error.message);
  }
}

async function deleteBook(book) {
  if (!window.confirm(`确定删除《${book.title || '未命名图书'}》及其所有图片吗？`)) return;
  try {
    await api.deleteBook(book.id);
    if (selected.value && selected.value.id === book.id) selected.value = null;
    await Promise.all([refreshBooks(), refreshStats()]);
    toast.success('已删除');
  } catch (error) {
    toast.error(error.message);
  }
}

async function findImages(book) {
  const target = book || selected.value;
  if (!target) return;
  if (!target.isbn && !target.title) {
    toast.error('先补全书名或 ISBN 再找图');
    return;
  }
  try {
    await api.setImportContext(target.id);
    const url = searchUrl(settings.value.kongfzUrl, target);
    await openBrowser('kongfz', url);
    toast.info('找图窗口已打开，选中图片后点工具条上的「导入」');
  } catch (error) {
    toast.error(error.message);
  }
}

async function openTaobao(book) {
  const target = book || selected.value;
  const url =
    target && target.itemId
      ? `https://item.taobao.com/item.htm?id=${target.itemId}`
      : 'https://www.taobao.com/';
  await openBrowser('taobao', url);
}

async function publish(payload) {
  if (!selected.value) return;
  publishing.value = true;
  try {
    selected.value = await api.updateBook(selected.value.id, payload.patch);
    const result = await api.saveListing(selected.value.id, {
      title: payload.title,
      description: payload.description,
      price: payload.price,
    });
    await openBrowser('goofish', result.publishUrl);
    await refreshSelected();
    await Promise.all([refreshBooks(true), refreshStats()]);
    toast.success(
      settings.value.autoSubmit
        ? '发布窗口已打开，脚本会自动填表并提交'
        : '发布窗口已打开，请检查内容后手动提交',
    );
  } catch (error) {
    toast.error(error.message);
  } finally {
    publishing.value = false;
  }
}

async function quickPublish(book) {
  if (!book.imageCount) {
    toast.error('至少需要一张图片，先点「找图」');
    return;
  }
  publishing.value = true;
  try {
    const result = await api.saveListing(book.id, {});
    await openBrowser('goofish', result.publishUrl);
    toast.success('发布窗口已打开，脚本会自动填表并提交');
    await refreshBooks(true);
  } catch (error) {
    toast.error(error.message);
  } finally {
    publishing.value = false;
  }
}

async function refreshActiveView() {
  if (activeView.value === 'xianyu') await xianyuWorkbench.value?.refresh();
  else if (activeView.value === 'books') await Promise.all([refreshBooks(), refreshStats()]);
  else await operations.value?.refresh();
}

function openApiPublish(book) {
  xianyuPublish.value = { bookId: book?.id };
}

async function apiPublishFromDrawer(payload) {
  if (!selected.value) return;
  const saved = await saveBook(payload.patch);
  if (!saved) return;
  xianyuPublish.value = { bookId: saved.id, initialCopy: payload };
  selected.value = null;
}

async function apiPublished(task) {
  xianyuPublish.value = null;
  activeView.value = 'xianyu';
  await Promise.all([refreshBooks(true), refreshStats()]);
  await nextTick();
  try {
    await xianyuWorkbench.value?.reloadTasks();
  } catch (error) {
    toast.error(error.message);
  }
  if (task.error) toast.error(task.error);
  else
    toast.success(
      task.status === 'created'
        ? '商品已创建，请在发布记录中核查图片并继续上架'
        : '请求已提交，请核查发布记录',
    );
}

async function uploadImages(files) {
  if (!selected.value) return;
  try {
    for (const file of files) {
      await api.uploadImage(selected.value.id, file, 'upload');
    }
    await refreshSelectedImages();
    await refreshBooks(true);
    toast.success(`已上传 ${files.length} 张图片`);
  } catch (error) {
    toast.error(error.message);
  }
}

async function importUrls(payload) {
  if (!selected.value) return;
  const urls = payload.urls || [];
  try {
    toast.info(`正在下载 ${urls.length} 张图片…`);
    const result = await api.importImages(selected.value.id, urls, payload.source || 'kongfz');
    await refreshSelectedImages();
    await refreshBooks(true);
    if (result.errors && result.errors.length) {
      toast.error(`${result.imported.length} 张成功，${result.errors.length} 张失败`);
    } else {
      toast.success(`已导入 ${result.imported.length} 张图片`);
    }
  } catch (error) {
    toast.error(error.message);
  }
}

async function setPrimaryImage(image) {
  try {
    await api.setPrimary(image.id);
    await refreshSelectedImages();
    await refreshBooks(true);
    toast.success('已设为主图');
  } catch (error) {
    toast.error(error.message);
  }
}

async function removeImage(image) {
  if (!window.confirm('删除这张图片？')) return;
  try {
    await api.deleteImage(image.id);
    await refreshSelectedImages();
    await refreshBooks(true);
    toast.success('图片已删除');
  } catch (error) {
    toast.error(error.message);
  }
}

function openEditor(image) {
  editorImage.value = image;
}

async function saveProcessed(blob) {
  if (!editorImage.value) return;
  try {
    await api.saveProcessed(editorImage.value.id, blob);
    editorImage.value = null;
    await refreshSelectedImages();
    await refreshBooks(true);
    toast.success('图片已更新，发布时会使用处理后的版本');
  } catch (error) {
    toast.error(error.message);
  }
}

async function saveSettings(patch) {
  try {
    settings.value = await api.saveSettings(patch);
    settingsOpen.value = false;
    toast.success('设置已保存');
  } catch (error) {
    toast.error(error.message);
  }
}

async function exportCsv() {
  try {
    const blob = await api.exportCsv();
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `淘书工坊-${new Date().toISOString().slice(0, 10)}.csv`;
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 4000);
    toast.success('已导出 CSV');
  } catch (error) {
    toast.error(error.message);
  }
}

onMounted(bootstrap);
onBeforeUnmount(stopPolling);
</script>

<template>
  <div class="shell">
    <TopBar
      :stats="stats"
      :loading="loading"
      @refresh="refreshActiveView"
      @taobao="openTaobao()"
      @kongfz="openBrowser('kongfz')"
      @goofish="openBrowser('goofish')"
      @settings="settingsOpen = true"
      @export="exportCsv"
    />

    <main class="content">
      <nav class="xy-view-tabs" aria-label="工作区切换">
        <UiButton
          v-for="(label, key) in {
            home: '今日待办',
            schools: '学校书单',
            books: '图书中心',
            sources: '实体货源',
            fulfillment: '订单履约',
            reports: '经营统计',
            xianyu: '销售渠道',
          }"
          :key="key"
          :class="{ active: activeView === key }"
          @click="activeView = key"
        >
          {{ label }}
        </UiButton>
      </nav>
      <XianyuWorkbench
        v-if="activeView === 'xianyu' && ready"
        ref="xianyuWorkbench"
        @publish="openApiPublish()"
        @open-book="selectBook"
        @updated="
          refreshBooks(true);
          refreshStats();
        "
      />
      <OperationsWorkspace
        v-if="ready && !['books', 'xianyu'].includes(activeView)"
        ref="operations"
        :view="activeView"
        @open-book="selectBook"
        @navigate="navigateOperations"
        @updated="
          refreshBooks(true);
          refreshStats();
        "
      />
      <template v-if="activeView === 'books'">
        <StatsRow :stats="stats" />

        <section class="panel">
          <div class="panel-head">
            <div class="filters">
              <div class="search-wrap">
                <span class="search-icon">⌕</span>
                <input
                  v-model="query"
                  class="input search"
                  type="search"
                  placeholder="搜索书名 / 作者 / 出版社 / ISBN"
                />
              </div>
              <SearchSelect v-model="statusFilter" class="input select" :aria-label="filterLabel">
                <option value="">全部状态</option>
                <option value="draft">待整理</option>
                <option value="ready">待发布</option>
                <option value="listed">已上架</option>
                <option value="sold">已售出</option>
              </SearchSelect>
            </div>
            <div class="head-actions">
              <UiButton class="btn ghost" @click="createManual">手动新增</UiButton>
              <UiButton
                class="btn ghost"
                :disabled="loading"
                @click="
                  refreshBooks();
                  refreshStats();
                "
              >
                {{ loading ? '刷新中…' : '刷新' }}
              </UiButton>
            </div>
          </div>

          <div class="business-filters">
            <SearchSelect v-model="schoolFilter" class="input" aria-label="适用学校">
              <option value="">全部学校</option>
              <option v-for="n in bookSchools" :key="n">{{ n }}</option>
            </SearchSelect>
            <SearchSelect v-model="yearFilter" class="input" aria-label="适用年份">
              <option value="">全部年份</option>
              <option v-for="n in bookYears" :key="n" :value="n">{{ n }}</option>
            </SearchSelect>
            <input
              v-model="majorFilter"
              class="input"
              placeholder="筛选适用专业"
              aria-label="适用专业"
            />
            <SearchSelect v-model="resourceFilter" class="input" aria-label="经营准备状态">
              <option value="">全部准备状态</option>
              <option value="physical">缺实体货源</option>
              <option value="digital">缺电子资源</option>
              <option value="complete">资源和货源齐全</option>
              <option value="listed">渠道已上架</option>
            </SearchSelect>
          </div>
          <BookTable
            :books="filteredBooks"
            :loading="loading"
            @select="selectBook"
            @find="findImages"
            @publish="quickPublish"
            @api-publish="openApiPublish"
            @delete="deleteBook"
          />
        </section>

        <footer class="footnote">
          <span>数据保存在本机 SQLite：<code>%APPDATA%\com.taoshu.studio\shelf.db</code></span>
          <span>学校用书 → 标准书目 → 货源与资源 → 渠道销售 → 订单履约</span>
        </footer>
      </template>
    </main>

    <Transition name="slide">
      <BookDrawer
        v-if="selected"
        :book="selected"
        :settings="settings"
        :publishing="publishing"
        @close="selected = null"
        @save="saveBook"
        @find="findImages(selected)"
        @publish="publish"
        @api-publish="apiPublishFromDrawer"
        @upload="uploadImages"
        @import-urls="importUrls"
        @primary="setPrimaryImage"
        @delete-image="removeImage"
        @edit="openEditor"
        @open-taobao="openTaobao(selected)"
        @refresh="
          refreshSelected();
          refreshBooks(true);
        "
        @open-order="openBookOrder"
        @open-fulfillment="
          (id) => {
            selected = null;
            navigateOperations('fulfillment', id);
          }
        "
      />
    </Transition>

    <ImageEditor
      v-if="editorImage"
      :image="editorImage"
      @close="editorImage = null"
      @saved="saveProcessed"
    />

    <SettingsModal
      v-if="settingsOpen"
      :settings="settings"
      @close="settingsOpen = false"
      @save="saveSettings"
    />

    <XianyuPublish
      v-if="xianyuPublish"
      v-bind="xianyuPublish"
      @close="xianyuPublish = null"
      @created="apiPublished"
    />

    <ToastStack :toasts="toasts" @remove="remove" />

    <div v-if="!ready && !bootError" class="boot">
      <div class="spinner"></div>
      <p>正在连接本地服务…</p>
    </div>

    <div v-if="bootError" class="boot error">
      <h2>启动失败</h2>
      <p>{{ bootError }}</p>
      <UiButton class="btn" @click="bootstrap">重试</UiButton>
    </div>
  </div>
</template>
