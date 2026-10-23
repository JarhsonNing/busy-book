<script setup>
defineProps({ toasts: { type: Array, default: () => [] } });
defineEmits(['remove']);
</script>

<template>
  <div class="toasts">
    <TransitionGroup name="toast">
      <div
        v-for="toast in toasts"
        :key="toast.id"
        :class="['toast', toast.type]"
        @click="$emit('remove', toast.id)"
      >
        <span class="toast-mark"></span>
        <span>{{ toast.message }}</span>
      </div>
    </TransitionGroup>
  </div>
</template>
