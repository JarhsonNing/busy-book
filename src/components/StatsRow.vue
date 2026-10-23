<script setup>
import { computed } from 'vue';

const props = defineProps({
  stats: { type: Object, default: () => ({}) },
});

const cards = computed(() => [
  {
    label: '标准书目',
    value: props.stats.total ?? 0,
    unit: '本',
    hint: `今日采集 ${props.stats.capturedToday ?? 0} 条货源`,
  },
  {
    label: '待整理 / 待发布',
    value: `${props.stats.draft ?? 0} / ${props.stats.ready ?? 0}`,
    unit: '',
    hint: '确认 ISBN、版次与册次',
  },
  {
    label: '已上架',
    value: props.stats.listed ?? 0,
    unit: '本',
    hint: `在售标价 ¥${(props.stats.listedValue ?? 0).toFixed(0)}`,
  },
  {
    label: '图片素材',
    value: props.stats.images ?? 0,
    unit: '张',
    hint: `采购与毛利请查看经营统计`,
  },
]);
</script>

<template>
  <section class="stats">
    <div v-for="card in cards" :key="card.label" class="stat-card">
      <span class="stat-label">{{ card.label }}</span>
      <div class="stat-value">
        {{ card.value }}<small v-if="card.unit">{{ card.unit }}</small>
      </div>
      <span class="stat-hint">{{ card.hint }}</span>
    </div>
  </section>
</template>
