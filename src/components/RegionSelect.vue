<script setup>
import { computed } from 'vue';
import { provinces, citiesFor, districtsFor, selectProvince, selectCity } from '../regions';

const props = defineProps({ modelValue: { type: Object, required: true } });
const emit = defineEmits(['update:modelValue']);
const cities = computed(() => citiesFor(props.modelValue.province));
const districts = computed(() => districtsFor(props.modelValue.province, props.modelValue.city));
</script>

<template>
  <div class="xy-region">
    <label class="field">
      <span>省份</span>
      <select
        :value="modelValue.province"
        class="input"
        required
        @change="emit('update:modelValue', selectProvince($event.target.value))"
      >
        <option value="">请选择省份</option>
        <option v-for="province in provinces" :key="province.id" :value="province.id">
          {{ province.name }}
        </option>
      </select>
    </label>
    <label class="field">
      <span>城市</span>
      <select
        :value="modelValue.city"
        class="input"
        required
        :disabled="!cities.length"
        @change="emit('update:modelValue', selectCity(modelValue.province, $event.target.value))"
      >
        <option value="">请选择城市</option>
        <option v-for="city in cities" :key="city.id" :value="city.id">{{ city.name }}</option>
      </select>
    </label>
    <label class="field">
      <span>区县</span>
      <select
        :value="modelValue.district"
        class="input"
        required
        :disabled="!districts.length"
        @change="
          emit('update:modelValue', {
            province: modelValue.province,
            city: modelValue.city,
            district: $event.target.value ? Number($event.target.value) : '',
          })
        "
      >
        <option value="">请选择区县</option>
        <option v-for="district in districts" :key="district.id" :value="district.id">
          {{ district.name }}
        </option>
      </select>
    </label>
  </div>
</template>
