<script setup>
import { assetUrl } from '../api';

defineProps({
  books: { type: Array, default: () => [] },
  loading: Boolean,
});
defineEmits(['select', 'find', 'publish', 'api-publish', 'delete']);

const statusText = {
  draft: '待整理',
  ready: '待发布',
  listed: '已上架',
  sold: '已售出',
};

function money(value) {
  if (value === null || value === undefined || value === '') return '—';
  return `¥${Number(value).toFixed(2)}`;
}
</script>

<template>
  <div class="table-wrap">
    <table class="book-table">
      <thead>
        <tr>
          <th class="col-cover"></th>
          <th>图书</th>
          <th class="col-isbn">ISBN</th>
          <th class="col-money">成本</th>
          <th class="col-money">售价</th>
          <th class="col-images">图片</th>
          <th class="col-status">状态</th>
          <th class="col-actions">操作</th>
        </tr>
      </thead>
      <tbody>
        <tr v-if="!books.length && !loading">
          <td colspan="8" class="empty">
            <div class="empty-art">《 》</div>
            <p>还没有书目</p>
            <span>打开淘宝采集窗口，在商品页点「抓取本页」</span>
          </td>
        </tr>
        <tr v-for="book in books" :key="book.id" class="book-row" @click="$emit('select', book.id)">
          <td class="col-cover">
            <div class="cover">
              <img v-if="book.coverUrl" :src="assetUrl(book.coverUrl)" alt="" loading="lazy" />
              <span v-else class="cover-ph">书</span>
            </div>
          </td>
          <td>
            <div class="book-title">{{ book.title || '未命名图书' }}</div>
            <div class="book-meta">
              <span v-if="book.author">{{ book.author }}</span>
              <span v-if="book.publisher">{{ book.publisher }}</span>
              <span v-if="book.condition" class="chip tiny">{{ book.condition }}</span>
            </div>
          </td>
          <td class="col-isbn mono">{{ book.isbn || '—' }}</td>
          <td class="col-money">{{ money(book.costPrice) }}</td>
          <td class="col-money strong">{{ money(book.sellPrice) }}</td>
          <td class="col-images">
            <span :class="['img-count', { zero: !book.imageCount }]">{{ book.imageCount }}</span>
          </td>
          <td class="col-status">
            <span :class="['status', book.status]">{{
              statusText[book.status] || book.status
            }}</span>
            <p class="hint" v-if="book.physicalReady !== undefined">
              货源{{ book.physicalReady ? ' ✓' : ' 待备' }} · 电子{{
                book.digitalReady ? ' ✓' : ' 待备'
              }}
            </p>
          </td>
          <td class="col-actions" @click.stop>
            <UiButton class="btn tiny ghost" @click="$emit('find', book)">找图</UiButton>
            <UiButton
              class="btn tiny"
              :disabled="!book.imageCount"
              :title="book.imageCount ? '一键发布到闲鱼' : '先找图'"
              @click="$emit('publish', book)"
            >
              网页发布
            </UiButton>
            <UiButton class="btn tiny ghost" @click="$emit('api-publish', book)">接口发布</UiButton>
            <UiButton class="icon-btn tiny danger" title="删除" @click="$emit('delete', book)">
              ×
            </UiButton>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
