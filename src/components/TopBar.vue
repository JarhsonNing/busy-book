<script setup>
import {Settings, RefreshCw, Download} from '@lucide/vue';
defineProps({
  stats: { type: Object, default: () => ({}) },
  loading: Boolean,
});
defineEmits(['refresh', 'taobao', 'kongfz', 'goofish', 'settings', 'export']);
</script>

<template>
  <header class="topbar">
    <div class="brand">
      <div class="brand-mark">书</div>
      <div class="brand-text">
        <h1>淘书工坊</h1>
        <p>以书为中心 · 选品、销售与履约</p>
      </div>
    </div>

    <nav class="site-nav">
      <UiButton class="site-link taobao" @click="$emit('taobao')">
        <span class="dot"></span>淘宝货源
      </UiButton>
      <UiButton class="site-link kongfz" @click="$emit('kongfz')">
        <span class="dot"></span>孔夫子找图
      </UiButton>
      <UiButton class="site-link goofish" @click="$emit('goofish')">
        <span class="dot"></span>闲鱼渠道
      </UiButton>
    </nav>

    <div class="top-actions">
      <span class="live"><span class="live-dot"></span>本地库 {{ stats.total || 0 }} 本</span>
      <UiButton class="btn ghost small" @click="$emit('export')"><Download :size="15" aria-hidden="true" />导出 CSV</UiButton>
      <UiButton class="icon-btn" title="设置" @click="$emit('settings')"><Settings :size="17" aria-hidden="true" /></UiButton>
      <UiButton class="btn small" :disabled="loading" @click="$emit('refresh')">
        <RefreshCw :size="15" :class="{ spinning: loading }" aria-hidden="true" />{{ loading ? '刷新中' : '刷新' }}
      </UiButton>
    </div>
  </header>
</template>
