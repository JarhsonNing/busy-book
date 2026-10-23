<script setup>
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { api, assetUrl, openBrowser } from '../api';
import { imageModes, listOf, normalizeIsbn, validIsbn, weightedLength } from '../xianyu';
import RegionSelect from './RegionSelect.vue';
import { isValidRegion, regionVersion, selectProvince } from '../regions';
const props = defineProps({ bookId: Number, initialCopy: Object });
const emit = defineEmits(['close', 'created']);
const books = ref([]),
  shops = ref([]),
  categories = ref([]),
  addresses = ref([]),
  images = ref([]);
const selectedId = ref(props.bookId || ''),
  addressIndex = ref('');
const error = ref(''),
  busy = ref(false),
  loading = ref(true),
  propertiesLoading = ref(false);
const propertiesError = ref(false);
const nodes = ref([]),
  choices = reactive({});
let bookVersion = 0,
  categoryVersion = 0;
const form = reactive({
  saleMode: 'physical',
  userName: '',
  channelCatId: '',
  province: '',
  city: '',
  district: '',
  title: '',
  isbn: '',
  description: '',
  price: '',
  expressFee: 0,
  stock: 1,
  stuffStatus: 90,
  imageMode: 'data-url',
  imageUrls: '',
  createOnly: true,
  firstImageOnly: true,
});
const availableShops = computed(() =>
  shops.value.filter(
    (s) =>
      s.is_valid !== false &&
      (!s.authorize_expires || Number(s.authorize_expires) > Date.now() / 1000),
  ),
);
const imageCount = computed(() =>
  form.imageMode === 'url'
    ? form.imageUrls.split(/\s+/).filter(Boolean).length
    : Math.min(images.value.length, form.firstImageOnly ? 1 : 9),
);
const validProperties = computed(
  () => !propertiesError.value && nodes.value.every((n) => !n.required || choices[n.property_id]),
);
async function loadBook() {
  const version = ++bookVersion;
  images.value = [];
  form.isbn = '';
  if (!selectedId.value) return;
  loading.value = true;
  error.value = '';
  try {
    const [book, copy] = await Promise.all([
      api.book(selectedId.value),
      api.xianyuPreview(selectedId.value),
    ]);
    if (version !== bookVersion) return;
    images.value = book.images || [];
    const override = Number(selectedId.value) === props.bookId ? props.initialCopy : null;
    Object.assign(form, {
      title: override?.title ?? copy.title,
      isbn: book.isbn || '',
      description: override?.description ?? copy.description,
      price: override?.price ?? copy.price,
      stock: copy.stock || 1,
      stuffStatus:
        {
          全新: 100,
          十成新: 99,
          九五新: 95,
          九成新: 90,
          八成新: 80,
          七成新: 70,
          六成新: 60,
          五成新: 50,
        }[copy.condition] ?? 90,
    });
  } catch (e) {
    if (version === bookVersion) error.value = e.message;
  } finally {
    if (version === bookVersion) loading.value = false;
  }
}
watch(selectedId, loadBook);
function descendants(id) {
  const ids = new Set([id]);
  for (let changed = true; changed; ) {
    changed = false;
    for (const n of nodes.value)
      if (ids.has(n.parent) && !ids.has(n.property_id)) {
        ids.add(n.property_id);
        changed = true;
      }
  }
  return ids;
}
async function chooseProperty(node) {
  const removed = descendants(node.property_id);
  removed.delete(node.property_id);
  for (const id of removed) delete choices[id];
  nodes.value = nodes.value.filter((n) => !removed.has(n.property_id));
  const item = node.items.find((i) => i.value_id === choices[node.property_id]);
  if (!item?.sub_property_id) return;
  const version = categoryVersion;
  propertiesLoading.value = true;
  propertiesError.value = false;
  error.value = '';
  try {
    const data = await api.xianyuCall('properties', {
      item_biz_type: 2,
      sp_biz_type: 24,
      channel_cat_id: form.channelCatId,
      sub_property_id: item.sub_property_id,
    });
    if (version !== categoryVersion || choices[node.property_id] !== item.value_id) return;
    for (const p of listOf(data)) {
      if (!nodes.value.some((n) => n.property_id === p.property_id))
        nodes.value.push({ ...p, parent: node.property_id });
    }
  } catch (e) {
    propertiesError.value = true;
    error.value = e.message;
  } finally {
    propertiesLoading.value = false;
  }
}
watch(
  () => form.channelCatId,
  async (category) => {
    const version = ++categoryVersion;
    nodes.value = [];
    for (const key of Object.keys(choices)) delete choices[key];
    if (!category) return;
    propertiesLoading.value = true;
    error.value = '';
    try {
      const data = await api.xianyuCall('properties', {
        item_biz_type: 2,
        sp_biz_type: 24,
        channel_cat_id: category,
      });
      if (version === categoryVersion)
        nodes.value = listOf(data).map((n) => ({ ...n, parent: null }));
    } catch (e) {
      if (version === categoryVersion) {
        propertiesError.value = true;
        error.value = e.message;
      }
    } finally {
      if (version === categoryVersion) propertiesLoading.value = false;
    }
  },
);
watch(addressIndex, (index) => {
  const a = addresses.value[Number(index)];
  if (index === '' || !a) return;
  if (isValidRegion(a)) {
    Object.assign(form, {
      province: Number(a.province),
      city: Number(a.city),
      district: Number(a.district),
    });
  } else {
    Object.assign(form, selectProvince(''));
    error.value = '商家地址的地区编码不在当前地区表中，请重新选择省、市、区。';
  }
});
function chooseRegion(value) {
  addressIndex.value = '';
  Object.assign(form, value);
}
async function submit() {
  if (form.saleMode === 'mixed') {
    error.value = '双规格托管自动交付尚未验证，请在图书经营中保存混合商品草稿';
    return;
  }
  if (busy.value || loading.value || propertiesLoading.value) return;
  error.value = '';
  if (!selectedId.value || !form.userName || !form.channelCatId) {
    error.value = '请选择本地图书、店铺和图书类目';
    return;
  }
  if (!validIsbn(form.isbn)) {
    error.value = '请补填有效的 ISBN：10 位，或以 978/979 开头的 13 位';
    return;
  }
  if (!imageCount.value) {
    error.value = '至少需要一张图片';
    return;
  }
  if (!isValidRegion(form)) {
    error.value = '请选择完整的省、市、区';
    return;
  }
  if (!validProperties.value) {
    error.value = '请填写必选商品属性';
    return;
  }
  if (
    !form.title.trim() ||
    weightedLength(form.title) > 60 ||
    weightedLength(form.description) < 5 ||
    weightedLength(form.description) > 5000
  ) {
    error.value = '请检查标题和描述的字符数';
    return;
  }
  const channelPv = nodes.value
    .filter((n) => choices[n.property_id])
    .map((n) => {
      const item = n.items.find((i) => i.value_id === choices[n.property_id]);
      return {
        property_id: n.property_id,
        property_name: n.property_name,
        value_id: item.value_id,
        value_name: item.value_name,
      };
    });
  busy.value = true;
  try {
    const task = await api.xianyuPublish(Number(selectedId.value), {
      ...form,
      isbn: normalizeIsbn(form.isbn),
      province: Number(form.province),
      city: Number(form.city),
      district: Number(form.district),
      price: Number(form.price),
      expressFee: Number(form.expressFee),
      stock: Number(form.stock),
      channelPv,
      imageUrls: form.imageUrls.split(/\s+/).filter(Boolean),
    });
    emit('created', task);
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
onMounted(async () => {
  try {
    const [localBooks, shopData, catData, hosting] = await Promise.all([
      api.books({ limit: 1000 }),
      api.xianyuCall('shops'),
      api.xianyuCall('categories', { item_biz_type: 2, sp_biz_type: 24 }),
      api.giteeConfig(),
    ]);
    if (hosting.tokenConfigured) form.imageMode = 'gitee';
    books.value = localBooks;
    shops.value = listOf(shopData);
    categories.value = listOf(catData);
    form.userName = availableShops.value[0]?.user_name || '';
    if (selectedId.value) await loadBook();
    try {
      addresses.value = listOf(await api.xianyuCall('addresses', { type: 1 }));
      const preferred = addresses.value.findIndex((a) => a.is_def);
      if (addresses.value.length && !form.province)
        addressIndex.value = String(preferred >= 0 ? preferred : 0);
    } catch {
      /* 地址接口不可用时，仍可使用内置省市区选择。 */
    }
  } catch (e) {
    error.value = e.message;
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <div class="xy-overlay" @click.self="!busy && emit('close')">
    <section
      class="xy-dialog xy-publisher"
      role="dialog"
      aria-modal="true"
      aria-labelledby="xy-publish-title"
    >
      <header class="xy-dialog-head">
        <div>
          <span class="xy-eyebrow">本地图书 → 闲鱼</span>
          <h2 id="xy-publish-title">接口发布</h2>
        </div>
        <UiButton class="icon-btn" :disabled="busy" aria-label="关闭" @click="emit('close')">
          ×
        </UiButton>
      </header>
      <form @submit.prevent="submit">
        <div class="xy-dialog-body">
          <p class="xy-notice">
            Gitee 模式会上传本地图片，验证匿名直链后再创建商品。也可选择 Base64 实验模式；明确拒绝
            Data URL 时会尝试纯 Base64。
          </p>
          <label class="field"
            ><span>售卖形态</span
            ><SearchSelect v-model="form.saleMode" class="input">
              <option value="physical">实体单规格（兼容现有流程）</option>
              <option value="mixed">电子＋实体双规格（待验证）</option>
            </SearchSelect></label
          >
          <p v-if="form.saleMode === 'mixed'" class="xy-notice">
            双规格托管与关机发货尚待验证，请到图书经营的「销售商品」准备草稿。
          </p>
          <p v-if="error" class="xy-error" role="alert">{{ error }}</p>
          <p v-if="loading" class="hint">正在载入发布资料…</p>
          <fieldset :disabled="busy || loading" class="xy-fieldset form-grid two">
            <label class="field"
              ><span>本地图书</span
              ><SearchSelect v-model="selectedId" required class="input">
                <option value="">选择图书</option>
                <option v-for="book in books" :key="book.id" :value="book.id">
                  #{{ book.id }} {{ book.title }}
                </option>
              </SearchSelect></label
            >
            <label class="field"
              ><span>发布店铺</span
              ><SearchSelect v-model="form.userName" required class="input">
                <option value="">选择有效店铺</option>
                <option
                  v-for="shop in availableShops"
                  :key="shop.user_name"
                  :value="shop.user_name"
                >
                  {{ shop.shop_name || shop.user_nick }} · {{ shop.user_name }}
                </option>
              </SearchSelect></label
            >
            <label class="field"
              ><span>图书类目</span
              ><SearchSelect
                v-model="form.channelCatId"
                class="input"
                required
                :disabled="propertiesLoading"
              >
                <option value="">选择类目</option>
                <option
                  v-for="cat in categories"
                  :key="cat.channel_cat_id"
                  :value="cat.channel_cat_id"
                >
                  {{ cat.channel_cat_name }}
                </option>
              </SearchSelect></label
            >
            <label class="field"
              ><span>成色</span
              ><SearchSelect v-model="form.stuffStatus" class="input">
                <option
                  v-for="[value, label] in [
                    [100, '全新'],
                    [99, '99 新'],
                    [95, '95 新'],
                    [90, '9 新'],
                    [80, '8 新'],
                    [70, '7 新'],
                    [60, '6 新'],
                    [50, '5 新及以下'],
                    [0, '无成色'],
                  ]"
                  :key="value"
                  :value="value"
                >
                  {{ label }}
                </option>
              </SearchSelect></label
            >
            <label v-for="node in nodes" :key="node.property_id" class="field"
              ><span>{{ node.property_name }}{{ node.required ? ' *' : '' }}</span
              ><SearchSelect
                v-model="choices[node.property_id]"
                class="input"
                :required="!!node.required"
                :disabled="propertiesLoading"
                @change="chooseProperty(node)"
              >
                <option value="">请选择</option>
                <option v-for="item in node.items" :key="item.value_id" :value="item.value_id">
                  {{ item.value_name }}
                </option>
              </SearchSelect></label
            >
            <p v-if="propertiesLoading" class="hint span-2">正在加载类目属性…</p>
            <label v-if="addresses.length" class="field span-2"
              ><span>使用商家地址的地区</span
              ><SearchSelect v-model="addressIndex" class="input">
                <option value="">手动选择省市区</option>
                <option
                  v-for="(address, i) in addresses"
                  :key="address.address_id"
                  :value="String(i)"
                >
                  {{ address.region }} {{ address.address }}{{ address.is_def ? '（默认）' : '' }}
                </option>
              </SearchSelect></label
            >
            <RegionSelect :model-value="form" class="span-2" @update:model-value="chooseRegion" />
            <p class="hint span-2">
              使用闲管家地区表（{{ regionVersion }}），选择后自动填写编码，也可从商家地址载入。
            </p>
            <label class="field span-2">
              <span>ISBN（必填）</span>
              <input
                v-model="form.isbn"
                class="input mono"
                required
                placeholder="例如：9787505720176"
              />
              <small class="hint"
                >从图书资料带入，可在此补填本次发布使用的 ISBN；支持 10 位或 13
                位，可带空格和连字符。</small
              >
            </label>
            <label class="field span-2"
              ><span
                >标题 <em>{{ weightedLength(form.title) }}/60（中文计 2）</em></span
              ><input v-model="form.title" class="input" required
            /></label>
            <label class="field"
              ><span>售价（元）</span
              ><input
                v-model="form.price"
                type="number"
                step="0.01"
                min="0.01"
                required
                class="input"
            /></label>
            <label class="field"
              ><span>运费（元）</span
              ><input
                v-model="form.expressFee"
                type="number"
                step="0.01"
                min="0"
                required
                class="input"
            /></label>
            <label class="field"
              ><span>库存</span
              ><input v-model="form.stock" type="number" min="1" max="9999" required class="input"
            /></label>
            <label class="field"
              ><span>图片方式</span
              ><SearchSelect v-model="form.imageMode" class="input">
                <option v-for="(label, value) in imageModes" :key="value" :value="value">
                  {{ label }}
                </option>
              </SearchSelect></label
            >
            <label class="field span-2"
              ><span
                >描述 <em>{{ weightedLength(form.description) }}/5000</em></span
              ><textarea v-model="form.description" class="input textarea" rows="7" required />
            </label>
            <label v-if="form.imageMode === 'url'" class="field span-2"
              ><span>公网图片 URL（一行一个，最多 9 张）</span
              ><textarea
                v-model="form.imageUrls"
                class="input textarea"
                rows="3"
                placeholder="https://…"
              />
            </label>
            <div v-else class="span-2">
              <div class="xy-preview-images">
                <img
                  v-for="image in images.slice(0, form.firstImageOnly ? 1 : 9)"
                  :key="image.id"
                  :src="assetUrl(image.processed ? image.processedUrl : image.url)"
                  :alt="image.processed ? '已净化图片' : '原始图片'"
                />
              </div>
              <p class="hint">优先使用净化图，第一张为主图。本次发送 {{ imageCount }} 张。</p>
              <label class="xy-check"
                ><input v-model="form.firstImageOnly" type="checkbox" />先只测试首图</label
              >
            </div>
            <label class="xy-check span-2"
              ><input
                v-model="form.createOnly"
                type="checkbox"
              />只创建，稍后在发布记录中上架</label
            >
          </fieldset>
        </div>
        <footer class="xy-dialog-foot">
          <UiButton type="button" class="btn ghost" :disabled="busy" @click="openBrowser('goofish')">
            打开闲鱼网页
          </UiButton>
          <div class="spacer"></div>
          <UiButton type="button" class="btn ghost" :disabled="busy" @click="emit('close')">
            取消</UiButton
          ><UiButton
            class="btn accent"
            :disabled="
              busy || loading || propertiesLoading || !validProperties || form.saleMode === 'mixed'
            "
          >
            {{ busy ? '提交中，请稍候…' : form.createOnly ? '创建并测试图片' : '创建并上架' }}
          </UiButton>
        </footer>
      </form>
    </section>
  </div>
</template>
