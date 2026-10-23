<script setup>
import { computed, ref, useSlots, useAttrs, watch } from 'vue';
import {
  PopoverRoot,
  PopoverTrigger,
  PopoverPortal,
  PopoverContent,
  ComboboxRoot,
  ComboboxAnchor,
  ComboboxInput,
  ComboboxContent,
  ComboboxViewport,
  ComboboxItem,
} from 'reka-ui';
import { Check, ChevronsUpDown, Search } from '@lucide/vue';
import { collectOptions, filterOptions } from './select-options';
defineOptions({ inheritAttrs: false });
const props = defineProps({
  modelValue: { default: undefined },
  value: { default: undefined },
  disabled: Boolean,
  required: Boolean,
  name: String,
});
const emit = defineEmits(['update:modelValue', 'change']);
const slots = useSlots(),
  attrs = useAttrs(),
  open = ref(false),
  query = ref('');
const options = computed(() => collectOptions(slots.default?.()));
const currentValue = computed(() =>
  props.modelValue !== undefined ? props.modelValue : props.value,
);
const selected = computed(
  () =>
    options.value.find((o) => Object.is(o.value, currentValue.value)) ||
    options.value.find(
      (o) =>
        o.value != null &&
        currentValue.value != null &&
        String(o.value) === String(currentValue.value),
    ),
);
const filtered = computed(() => filterOptions(options.value, query.value));
const selectedKey = computed(() =>
  selected.value ? String(options.value.indexOf(selected.value)) : undefined,
);
watch(open, () => {
  query.value = '';
});
function choose(key) {
  const option = options.value[Number(key)];
  if (!option || option.disabled) return;
  emit('update:modelValue', option.value);
  emit('change', { target: { value: option.value } });
  open.value = false;
}
</script>
<template>
  <PopoverRoot v-model:open="open">
    <PopoverTrigger as-child>
      <button
        v-bind="attrs"
        type="button"
        class="search-select input"
        role="combobox"
        aria-haspopup="listbox"
        :aria-expanded="open"
        :aria-required="required || undefined"
        :disabled="disabled"
        :title="selected?.label"
      >
        <span>{{ selected?.label || '请选择' }}</span
        ><ChevronsUpDown :size="15" aria-hidden="true" />
      </button>
    </PopoverTrigger>
    <input
      v-if="required"
      class="select-validation"
      tabindex="-1"
      aria-hidden="true"
      :name="name"
      :disabled="disabled"
      required
      :value="!selected || currentValue == null ? '' : String(currentValue)"
      @invalid.prevent="open = true"
    />
    <PopoverPortal>
      <PopoverContent class="select-popover" align="start" :side-offset="5" :collision-padding="12">
        <ComboboxRoot
          :model-value="selectedKey"
          :open="true"
          :ignore-filter="true"
          @update:model-value="choose"
        >
          <ComboboxAnchor class="select-search"
            ><Search :size="16" aria-hidden="true" /><ComboboxInput
              v-model="query"
              :display-value="() => ''"
              @keydown.esc.capture.stop.prevent="open = false"
              aria-label="搜索选项"
              placeholder="输入名称或编号搜索…"
              autocomplete="off"
          /></ComboboxAnchor>
          <ComboboxContent>
            <ComboboxViewport class="select-options" role="listbox">
              <ComboboxItem
                v-for="option in filtered"
                :key="String(options.indexOf(option))"
                :value="String(options.indexOf(option))"
                :disabled="option.disabled"
                class="select-option"
              >
                <span>{{ option.label }}</span
                ><Check v-if="option === selected" :size="16" aria-hidden="true" />
              </ComboboxItem>
              <div v-if="!filtered.length" class="select-empty" role="status">
                没有匹配的选项，请尝试其他关键词
              </div>
            </ComboboxViewport>
          </ComboboxContent>
        </ComboboxRoot>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>
