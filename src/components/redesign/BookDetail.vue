<script setup>
import { computed, onMounted, reactive, ref, watch } from "vue";
import { api, assetUrl } from "../../api";
import { fulfillmentStates } from "../../commerce";
import BookOperations from "../BookOperations.vue";
const props = defineProps({ book: Object });
const emit = defineEmits([
  "updated",
  "find-images",
  "find-source",
  "edit-image",
  "publish",
  "open-order",
  "deleted",
]);
const tab = ref("info"),
  busy = ref(false),
  error = ref(""),
  saved = ref(""),
  business = ref(null),
  businessEditor = ref(null),
  resourcesOpen = ref(false),
  fileInput = ref(null),
  urlOpen = ref(false),
  urls = ref("");
const form = reactive({});
const dirty = computed(() => JSON.stringify(form) !== saved.value);
function readForm() {
  const b = props.book;
  Object.assign(form, {
    title: b.title,
    author: b.author,
    publisher: b.publisher,
    isbn: b.isbn,
    edition: b.edition,
    condition: b.condition,
    costPrice: b.costPrice,
    sellPrice: b.sellPrice,
    listPrice: b.listPrice,
    stock: b.stock,
    note: b.note,
  });
  saved.value = JSON.stringify(form);
}
watch(() => props.book.id, readForm, { immediate: true });
async function refresh() {
  try {
    business.value = await api.commerce("/books/" + props.book.id);
    await businessEditor.value?.refresh();
  } catch (e) {
    error.value = e.message;
  }
}
async function act(fn) {
  busy.value = true;
  error.value = "";
  try {
    await fn();
    emit("updated");
    await refresh();
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
async function save() {
  await act(async () => {
    await api.updateBook(props.book.id, form);
    saved.value = JSON.stringify(form);
  });
}
async function publish() {
  if (dirty.value) {
    await save();
    if (error.value) return;
  }
  emit("publish");
}
async function upload(e) {
  const files = Array.from(e.target.files || []);
  await act(async () => {
    for (const f of files) await api.uploadImage(props.book.id, f);
  });
  e.target.value = "";
}
async function importUrls() {
  await act(async () => {
    const r = await api.importImages(
      props.book.id,
      urls.value.split(/\s+/).filter(Boolean),
    );
    if (r.errors?.length)
      throw new Error(`${r.imported.length} 张成功，${r.errors.length} 张失败`);
    urls.value = "";
    urlOpen.value = false;
  });
}
async function removeImage(i) {
  if (confirm("删除这张图片？")) await act(() => api.deleteImage(i.id));
}
async function reorder(i, d) {
  const ids = props.book.images.map((i) => i.id);
  const idx = ids.indexOf(i.id);
  if (idx + d < 0 || idx + d >= ids.length) return;
  [ids[idx], ids[idx + d]] = [ids[idx + d], ids[idx]];
  await act(() => api.reorderImages(props.book.id, ids));
}
async function businessUpdated() {
  await refresh();
  emit("updated");
}
async function remove() {
  if (confirm(`删除《${props.book.title}》及其图片？`)) {
    busy.value = true;
    try {
      await api.deleteBook(props.book.id);
      emit("deleted");
    } catch (e) {
      error.value = e.message;
    } finally {
      busy.value = false;
    }
  }
}
function showTab(value) {
  tab.value = value;
}
defineExpose({ refresh, showTab });
onMounted(refresh);
</script>
<template>
  <section class="book-detail">
    <header class="book-detail-heading">
      <img v-if="book.coverUrl" :src="assetUrl(book.coverUrl)" alt="图书封面" />
      <div>
        <h2>{{ book.title }}</h2>
        <p class="muted">
          {{ book.author || "作者待补充" }} ·
          {{ book.edition || "版次待确认" }} · {{ book.isbn || "ISBN 待补充" }}
        </p>
        <span v-if="!business?.physicalReady" class="tag warning">缺货源</span>
        <span v-if="!book.imageCount" class="tag warning">缺图片</span>
      </div>
      <button class="btn primary push-right" :disabled="busy" @click="publish">
        发布
      </button>
    </header>
    <nav class="page-tabs" aria-label="图书详情">
      <button
        v-for="(label, key) in {
          info: '资料',
          sources: '货源',
          images: '图片',
          sales: '销售记录',
        }"
        :key="key"
        :class="{ active: tab === key }"
        @click="tab = key"
      >
        {{ label }}
      </button>
    </nav>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
    <div v-show="tab === 'info'" class="detail-columns">
      <section>
        <form @submit.prevent="save">
          <h3>基本资料</h3>
          <div class="form-grid">
            <label class="field wide"
              ><span>书名</span
              ><input v-model="form.title" class="input" required /></label
            ><label
              v-for="(label, key) in {
                author: '作者',
                publisher: '出版社',
                isbn: 'ISBN',
                edition: '版次',
                condition: '品相',
              }"
              :key="key"
              class="field"
              ><span>{{ label }}</span
              ><input v-model="form[key]" class="input" /></label
            ><label
              v-for="(label, key) in {
                costPrice: '参考成本',
                sellPrice: '参考售价',
                listPrice: '定价',
                stock: '本地库存',
              }"
              :key="key"
              class="field"
              ><span>{{ label }}</span
              ><input
                v-model.number="form[key]"
                class="input"
                type="number"
                min="0"
                :step="key === 'stock' ? 1 : 0.01" /></label
            ><label class="field wide"
              ><span>备注</span
              ><textarea v-model="form.note" class="input" rows="3" />
            </label>
          </div>
          <div class="row">
            <button class="btn primary" :disabled="busy || !dirty">
              保存资料</button
            ><button
              class="link-btn danger push-right"
              type="button"
              @click="remove"
            >
              删除图书
            </button>
          </div>
        </form>
        <details class="detail-section">
          <summary>版本核对</summary>
          <BookOperations
            :book-id="book.id"
            section="profile"
            @updated="businessUpdated"
          />
        </details>
        <details
          class="detail-section"
          @toggle="resourcesOpen = $event.target.open"
        >
          <summary>电子资源</summary>
          <BookOperations
            v-if="resourcesOpen"
            :book-id="book.id"
            section="resources"
            @updated="businessUpdated"
          />
        </details>
      </section>
      <aside class="book-usage">
        <h3>适用学校与专业</h3>
        <div
          v-for="r in business?.schoolRecords || []"
          :key="r.id"
          class="usage-row"
        >
          <strong>{{ r.data.school }}</strong>
          <p>{{ r.data.major }} · {{ r.data.year }}</p>
          <small>{{ r.data.subject }} · {{ r.data.stage }}</small>
          <details>
            <summary>用书依据</summary>
            <p>{{ r.data.raw }}</p>
            <a
              v-if="/^https?:/.test(r.data.sourceUrl)"
              :href="r.data.sourceUrl"
              target="_blank"
              rel="noreferrer"
              >官方来源</a
            >
          </details>
        </div>
        <p v-if="!business?.schoolRecords?.length" class="muted">暂无关联</p>
      </aside>
    </div>
    <BookOperations
      v-if="tab === 'sources'"
      ref="businessEditor"
      :book-id="book.id"
      section="sources"
      @updated="businessUpdated"
    />
    <section v-if="tab === 'images'">
      <div class="module-bar">
        <span>{{ book.images?.length || 0 }} 张图片</span>
        <div class="row">
          <button class="btn" @click="$emit('find-images')">孔夫子找图</button
          ><button class="btn" @click="fileInput.click()">上传图片</button
          ><button class="btn ghost" @click="urlOpen = !urlOpen">
            导入链接</button
          ><input
            type="file"
            hidden
            multiple
            accept="image/*"
            ref="fileInput"
            @change="upload"
          />
        </div>
      </div>
      <form v-if="urlOpen" class="inline-form" @submit.prevent="importUrls">
        <textarea
          v-model="urls"
          class="input"
          placeholder="每行一个图片链接"
          aria-label="图片链接"
          required
        /><button class="btn primary" :disabled="busy">导入</button>
      </form>
      <div class="image-grid">
        <article
          v-for="(i, index) in book.images"
          :key="i.id"
          class="image-tile"
        >
          <img
            :src="assetUrl(i.processedUrl || i.url)"
            :alt="'图书图片 ' + (index + 1)"
          />
          <div class="image-meta">
            <span class="tag" :class="i.isPrimary ? 'success' : 'neutral'">{{
              i.isPrimary ? "主图" : i.processed ? "已处理" : "原图"
            }}</span
            ><span class="muted">{{ i.width }} × {{ i.height }}</span>
          </div>
          <div class="image-actions">
            <button class="link-btn" @click="$emit('edit-image', i)">
              编辑</button
            ><button
              class="link-btn"
              :disabled="i.isPrimary || busy"
              @click="act(() => api.setPrimary(i.id))"
            >
              设主图</button
            ><button
              class="link-btn"
              :disabled="index === 0 || busy"
              @click="reorder(i, -1)"
            >
              前移</button
            ><button class="link-btn danger" @click="removeImage(i)">
              删除
            </button>
          </div>
        </article>
      </div>
      <div v-if="!book.images?.length" class="empty-state">
        暂无图片 <button class="btn" @click="$emit('find-images')">找图</button>
      </div>
    </section>
    <section v-if="tab === 'sales'">
      <h3>店铺商品</h3>
      <BookOperations
        ref="businessEditor"
        :book-id="book.id"
        section="channels"
        @updated="businessUpdated"
      />
      <details class="detail-section">
        <summary>网页发布记录 · {{ book.listings?.length || 0 }}</summary>
        <table class="data-table">
          <tbody>
            <tr v-for="l in book.listings" :key="l.id">
              <td>{{ l.title }}</td>
              <td>{{ l.status }}</td>
              <td>{{ l.createdAt }}</td>
            </tr>
          </tbody>
        </table>
      </details>
      <h3>关联订单</h3>
      <table class="data-table">
        <thead>
          <tr>
            <th>订单</th>
            <th>状态</th>
            <th>时间</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="o in business?.fulfillments || []" :key="o.id">
            <td>
              <button
                class="text-button"
                @click="$emit('open-order', o.orderNo)"
              >
                {{ o.orderNo }}
              </button>
            </td>
            <td>{{ fulfillmentStates[o.state] || o.state }}</td>
            <td>{{ o.updatedAt }}</td>
          </tr>
        </tbody>
      </table>
    </section>
  </section>
</template>
