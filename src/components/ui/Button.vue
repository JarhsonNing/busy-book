<script setup>
import { Primitive } from 'reka-ui';
import { cva } from 'class-variance-authority';
defineProps({
  variant: { type: String, default: 'default' },
  as: { type: String, default: 'button' },
  asChild: Boolean,
});
const variants = cva('ui-button', {
  variants: { variant: { default: '', outline: 'ghost', destructive: 'danger' } },
});
</script>
<template>
  <Primitive :as="as" :as-child="asChild" :class="variants({ variant })"><slot /></Primitive>
</template>
