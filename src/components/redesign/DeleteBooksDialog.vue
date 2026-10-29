<script setup>
import { computed, onMounted, ref } from "vue";
import { X } from "@lucide/vue";
import { api } from "../../api";
const props = defineProps({ ids: { type: Array, required: true } });
const emit = defineEmits(["close", "deleted"]);
const loading = ref(true), busy = ref(false), error = ref(""), items = ref([]), done = ref(0);
const allowed = computed(() => items.value.filter((b) => b.canDelete));
function close() { if (!busy.value) emit("close"); }
async function load() {
  loading.value = true;
  error.value = "";
  items.value = [];
  try { items.value = (await api.previewDeleteBooks(props.ids)).items; }
  catch (e) { error.value = e.message; }
  finally { loading.value = false; }
}
async function remove() {
  if (busy.value || !allowed.value.length) return;
  busy.value = true;
  error.value = "";
  try {
    const result = await api.deleteBooks(allowed.value.map((b) => b.id));
    done.value += result.deletedIds.length;
    if (result.deletedIds.length) emit("deleted", result.deletedIds);
    if (!result.skipped.length) emit("close");
    else items.value = result.skipped;
  } catch (e) {
    error.value = e.message;
    // A request may have committed before the connection failed. Recheck the
    // visible records before offering another destructive action.
    try { items.value = (await api.previewDeleteBooks(props.ids)).items; }
    catch { items.value = []; }
    emit("deleted", []);
  } finally { busy.value = false; }
}
onMounted(load);
</script>
<template>
  <div class="modal-backdrop" @click.self="close">
    <section v-modal-focus="close" class="modal-dialog delete-books-dialog" role="dialog" aria-modal="true" aria-labelledby="delete-books-title" aria-describedby="delete-books-note" :aria-busy="loading || busy">
      <header class="delete-books-heading">
        <h2 id="delete-books-title">删除图书</h2>
        <button class="icon-btn" aria-label="关闭删除确认" :disabled="busy" @click="close"><X :size="18" /></button>
      </header>
      <p id="delete-books-note" class="muted">删除后无法恢复，图片和本地草稿会一并移除。</p>
      <p v-if="loading" class="delete-books-feedback" role="status">正在检查…</p>
      <ul v-else-if="items.length" class="delete-books-list">
        <li v-for="b in items" :key="b.id"><strong>{{ b.title }}</strong><span :class="b.canDelete ? 'muted' : 'danger'">{{ b.canDelete ? '可删除' : b.reason }}</span></li>
      </ul>
      <p v-if="done" class="delete-books-feedback" role="status">已删除 {{ done }} 本，其余图书未删除。</p>
      <p v-if="error" class="error-panel" role="alert">{{ error }} <button v-if="!busy" class="link-btn" @click="load">重新检查</button></p>
      <footer class="modal-actions">
        <button class="btn push-right" :disabled="busy" @click="close">{{ allowed.length ? '取消' : '关闭' }}</button>
        <button v-if="allowed.length" class="btn delete-confirm" :disabled="busy || loading" @click="remove">{{ busy ? '正在删除…' : `删除 ${allowed.length} 本` }}</button>
      </footer>
    </section>
  </div>
</template>
