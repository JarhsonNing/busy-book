<script setup>
import { computed, nextTick, onMounted, ref } from "vue";
import {
  BookOpen,
  Store,
  Download,
  Settings,
  ArrowLeft,
  RefreshCw,
  Bot,
} from "@lucide/vue";
import { api, initApi, openBrowser, searchUrl, taobaoSourceUrl } from "./api";
import { useToast } from "./composables/toast";
import ToastStack from "./components/ToastStack.vue";
import ImageEditor from "./components/ImageEditor.vue";
import XianyuPublish from "./components/XianyuPublish.vue";
import XianyuWorkbench from "./components/XianyuWorkbench.vue";
import BookLibrary from "./components/redesign/BookLibrary.vue";
import BookDetail from "./components/redesign/BookDetail.vue";
import CollectionCenter from "./components/redesign/CollectionCenter.vue";
import SettingsPage from "./components/redesign/SettingsPage.vue";
import OpenCodePage from "./components/redesign/OpenCodePage.vue";
import QoderPage from "./components/redesign/QoderPage.vue";
import "./commerce.css";

const toast = useToast();
const active = ref("xianyu"),
  visited = ref(new Set(["xianyu"])),
  ready = ref(false),
  error = ref(""),
  loading = ref(false);
const selected = ref(null),
  image = ref(null),
  publishing = ref(null),
  publishBook = ref(null),
  settings = ref({});
const library = ref(null),
  collector = ref(null),
  commerce = ref(null),
  settingsPage = ref(null),
  bookDetail = ref(null);
const navigation = [
  { id: "xianyu", name: "闲管家", icon: Store },
  { id: "books", name: "图书", icon: BookOpen },
  { id: "collect", name: "采集", icon: Download },
  { id: "opencode", name: "OpenCode", icon: Bot },
  { id: "qoder", name: "Qoder CLI", icon: Bot },
  { id: "settings", name: "设置", icon: Settings },
];
const title = computed(
  () => navigation.find((n) => n.id === active.value)?.name,
);
function navigate(id) {
  selected.value = null;
  active.value = id;
  visited.value.add(id);
}
async function openAgentSettings(provider='opencode') {
  navigate("settings");
  await nextTick();
  settingsPage.value?.showAgents(provider);
}
async function openAiSettings(){navigate('settings');await nextTick();settingsPage.value?.showAi();}
async function aiProgress(id){navigate('collect');await nextTick();await collector.value?.refresh();if(id)await collector.value?.openJob(id);}
async function aiCreated(job){await aiProgress(job.id);toast.success('AI 任务已创建');}
async function openAiDraft(id,task){await openBook(id);await nextTick();bookDetail.value?.showTab(task==='sources'?'sources':'sales');}
async function bootstrap() {
  loading.value = true;
  error.value = "";
  try {
    await initApi();
    settings.value = await api.settings();
    ready.value = true;
  } catch (e) {
    error.value = e.message;
  } finally {
    loading.value = false;
  }
}
let listScroll = 0;
async function backBook() {
  selected.value = null;
  await nextTick();
  window.scrollTo(0, listScroll);
}
async function openBook(id) {
  try {
    if (!selected.value) listScroll = window.scrollY;
    selected.value = await api.book(id);
    await nextTick();
    window.scrollTo(0, 0);
  } catch (e) {
    toast.error(e.message);
  }
}
async function updated() {
  if (selected.value) selected.value = await api.book(selected.value.id);
  await library.value?.refresh();
}
async function refresh() {
  if (selected.value) {
    await updated();
    await bookDetail.value?.refresh();
  } else if (active.value === "books") await library.value?.refresh();
  else if (active.value === "collect") await collector.value?.refresh();
  else await commerce.value?.refresh();
}
async function findImages(book) {
  try {
    await api.setImportContext(book.id);
    await openBrowser("kongfz", searchUrl(settings.value.kongfzUrl, book));
  } catch (e) {
    toast.error(e.message);
  }
}
async function findSource(book) {
  try { await openBrowser("taobao", taobaoSourceUrl(book)); }
  catch (e) { toast.error(e.message); }
}
async function runBatch(kind, ids) {
  try {
    await api.commerce("/jobs", { kind, bookIds: ids });
    navigate("collect");
    await nextTick();
    await collector.value?.refresh();
    toast.success("任务已创建");
  } catch (e) {
    toast.error(e.message);
  }
}
async function choosePublish(mode) {
  const book = publishBook.value;
  publishBook.value = null;
  if (mode === "api") {
    publishing.value = { bookId: book.id };
    return;
  }
  try {
    const result = await api.saveListing(book.id, {});
    await openBrowser("goofish", result.publishUrl);
    await updated();
    toast.success("发布窗口已打开");
  } catch (e) {
    toast.error(e.message);
  }
}
async function published(task) {
  publishing.value = null;
  navigate("xianyu");
  await nextTick();
  await commerce.value?.reloadTasks();
  await library.value?.refresh();
  if (task.error) toast.error(task.error);
}
async function saveImage(blob) {
  try {
    await api.saveProcessed(image.value.id, blob);
    image.value = null;
    await updated();
    toast.success("图片已保存");
  } catch (e) {
    toast.error(e.message);
  }
}
async function openOrder(no) {
  navigate("xianyu");
  await nextTick();
  await commerce.value?.openOrder(no);
}
async function workItem(kind, item) {
  try {
    const book = await api.book(item.bookId);
    if (kind === "images") await findImages(book);
    else if (kind === "sources") {
      await openBook(book.id);
      await nextTick();
      bookDetail.value?.showTab("sources");
      await findSource(book);
    } else publishBook.value = book;
  } catch (e) {
    toast.error(e.message);
  }
}
async function settingsUpdated() {
  settings.value = await api.settings();
  await commerce.value?.reconnect();
}
onMounted(bootstrap);
</script>
<template>
  <div class="studio-app">
    <aside class="studio-sidebar">
      <div class="studio-brand">
        <BookOpen :size="23" /><strong>淘书工坊</strong>
      </div>
      <nav aria-label="主导航">
        <button
          v-for="n in navigation"
          :key="n.id"
          :aria-label="n.name"
          :class="{ active: active === n.id }"
          :aria-current="active === n.id ? 'page' : undefined"
          @click="navigate(n.id)"
        >
          <component :is="n.icon" :size="19" /><span>{{ n.name }}</span>
        </button>
      </nav>
      <div class="sidebar-bottom">
        <span class="connection-dot" :class="{ online: ready }"></span
        >{{ ready ? "本地已连接" : "连接中" }}
      </div>
    </aside>
    <div class="studio-main">
      <header class="studio-topbar">
        <div class="row">
          <button
            v-if="selected"
            class="icon-btn"
            aria-label="返回列表"
            @click="backBook"
          >
            <ArrowLeft :size="18" />
          </button>
          <h1>{{ selected ? "图书详情" : title }}</h1>
        </div>
        <button
          v-if="active !== 'opencode' || !ready"
          class="btn ghost"
          :disabled="loading"
          @click="ready ? refresh() : bootstrap()"
        >
          <RefreshCw :size="15" />刷新
        </button>
      </header>
      <main class="studio-content" :class="{ 'opencode-content': active === 'opencode' && !selected }">
        <div v-if="error" class="error-panel" role="alert">
          {{ error }} <button class="btn" @click="bootstrap">重试</button>
        </div>
        <div v-else-if="!ready" class="empty-state" role="status">
          正在连接…
        </div>
        <template v-else>
          <div v-show="!selected" :class="{ 'opencode-layout': active === 'opencode' }">
            <XianyuWorkbench
              ref="commerce"
              v-show="active === 'xianyu'"
              @open-book="openBook"
              @publish="navigate('books')"
              @settings="navigate('settings')"
              @updated="updated"
            />
            <BookLibrary
              v-if="visited.has('books')"
              v-show="active === 'books'"
              ref="library"
              @open-book="openBook"
              @publish="publishBook = $event"
              @batch="runBatch"
              @collect="navigate('collect')"
              @ai-created="aiCreated"
              @ai-settings="openAiSettings"
            />
            <CollectionCenter
              v-if="visited.has('collect')"
              v-show="active === 'collect'"
              :active="active === 'collect'"
              ref="collector"
              @open-book="openBook"
              @work-item="workItem"
              @open-draft="openAiDraft"
              @open-schools="
                navigate('books');
                nextTick(() => library?.showSchools());
              "
            />
            <SettingsPage
              v-if="visited.has('settings')"
              v-show="active === 'settings'"
              ref="settingsPage"
              :active="active === 'settings'"
              @updated="settingsUpdated"
              @open-opencode="navigate('opencode')"
              @open-qoder="navigate('qoder')"
            />
            <OpenCodePage
              v-if="visited.has('opencode')"
              v-show="active === 'opencode'"
              :active="active === 'opencode'"
              @settings="openAgentSettings"
            />
            <QoderPage v-if="visited.has('qoder')" v-show="active === 'qoder'" :active="active === 'qoder'" @settings="openAgentSettings('qoder')" @ai-settings="openAiSettings" />
          </div>
          <BookDetail
            v-if="selected"
            :key="selected.id"
            ref="bookDetail"
            :book="selected"
            @updated="updated"
            @ai-settings="openAiSettings"
            @ai-progress="aiProgress"
            @find-images="findImages(selected)"
            @find-source="findSource(selected)"
            @edit-image="image = $event"
            @publish="publishBook = selected"
            @open-order="openOrder"
            @deleted="
              selected = null;
              library?.refresh();
            "
          />
        </template>
      </main>
    </div>
    <div
      v-if="publishBook"
      class="modal-backdrop"
      @click.self="publishBook = null"
      @keydown.esc="publishBook = null"
    >
      <section
        class="modal compact-dialog"
        v-modal-focus="
          () => {
            publishBook = null;
          }
        "
        role="dialog"
        aria-modal="true"
        aria-labelledby="publish-title"
      >
        <h2 id="publish-title">发布图书</h2>
        <p>{{ publishBook.title }}</p>
        <div class="dialog-actions">
          <button class="btn" @click="choosePublish('api')">闲管家接口</button
          ><button class="btn ghost" @click="choosePublish('web')">
            闲鱼网页</button
          ><button class="btn ghost" @click="publishBook = null">取消</button>
        </div>
      </section>
    </div>
    <XianyuPublish
      v-if="publishing"
      :book-id="publishing.bookId"
      @close="publishing = null"
      @created="published"
    />
    <ImageEditor
      v-if="image"
      :image="image"
      @close="image = null"
      @saved="saveImage"
    />
    <ToastStack :toasts="toast.toasts.value" @remove="toast.remove" />
  </div>
</template>
