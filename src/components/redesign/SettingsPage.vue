<script setup>
import { onMounted, reactive, ref } from "vue";
import { api } from "../../api";
const emit = defineEmits(["updated"]);
const tab = ref("platform"),
  busy = ref(false),
  error = ref(""),
  message = ref(""),
  configured = ref(false),
  hostingConfigured = ref(false);
const credentials = reactive({ appKey: "", appSecret: "" }),
  hosting = reactive({
    repoUrl: "",
    token: "",
    branch: "",
    directory: "taoshu-images",
  }),
  settings = reactive({});
async function action(fn) {
  busy.value = true;
  error.value = "";
  message.value = "";
  try {
    await fn();
    message.value = "已保存";
    emit("updated");
  } catch (e) {
    error.value = e.message;
  } finally {
    busy.value = false;
  }
}
async function load() {
  try {
    const [c, h, s] = await Promise.all([
      api.xianyuConfig(),
      api.giteeConfig(),
      api.settings(),
    ]);
    credentials.appKey = c.appKey || "";
    configured.value = c.secretConfigured;
    Object.assign(hosting, {
      repoUrl: h.repoUrl || "",
      branch: h.branch || "",
      directory: h.directory || "taoshu-images",
    });
    hostingConfigured.value = h.tokenConfigured;
    Object.assign(settings, s);
  } catch (e) {
    error.value = e.message;
  }
}
async function saveConnection() {
  await action(async () => {
    await api.saveXianyuConfig({ ...credentials });
    credentials.appSecret = "";
    await load();
  });
}
async function test() {
  await action(async () => {
    await api.xianyuCall("shops");
  });
  if (!error.value) message.value = "连接成功";
}
async function saveHosting() {
  await action(async () => {
    await api.saveGiteeConfig({ ...hosting });
    hosting.token = "";
    await load();
  });
}
async function testHosting() {
  await action(() => api.testGitee());
  if (!error.value) message.value = "仓库连接正常";
}
onMounted(load);
</script>
<template>
  <section class="settings-page">
    <nav class="page-tabs" aria-label="设置分类">
      <button
        v-for="(label, key) in {
          platform: '平台连接',
          images: '图片托管',
          pricing: '定价与文案',
          app: '应用',
        }"
        :key="key"
        :class="{ active: tab === key }"
        @click="tab = key"
      >
        {{ label }}
      </button>
    </nav>
    <p v-if="error" class="error-panel" role="alert">{{ error }}</p>
    <p v-if="message" class="success-message" role="status">{{ message }}</p>
    <form
      v-if="tab === 'platform'"
      class="settings-form"
      @submit.prevent="saveConnection"
    >
      <div class="module-bar">
        <h2>闲管家</h2>
        <span class="tag" :class="configured ? 'success' : 'neutral'">{{
          configured ? "已配置" : "未配置"
        }}</span>
      </div>
      <label class="field"
        ><span>AppKey</span
        ><input
          v-model="credentials.appKey"
          class="input"
          required
          autocomplete="off" /></label
      ><label class="field"
        ><span>AppSecret</span
        ><input
          v-model="credentials.appSecret"
          type="password"
          class="input"
          :placeholder="configured ? '已保存，留空保留' : ''"
          autocomplete="new-password"
      /></label>
      <div class="row">
        <button class="btn primary" :disabled="busy">保存</button
        ><button
          class="btn"
          type="button"
          :disabled="busy || !configured"
          @click="test"
        >
          测试连接
        </button>
      </div>
      <details class="detail-section">
        <summary>店铺授权</summary>
        <p>在闲管家完成店铺授权后，在订单页切换店铺。</p>
      </details>
    </form>
    <form
      v-if="tab === 'images'"
      class="settings-form"
      @submit.prevent="saveHosting"
    >
      <div class="module-bar">
        <h2>Gitee 图片托管</h2>
        <span class="tag">{{ hostingConfigured ? "已配置" : "未配置" }}</span>
      </div>
      <label
        v-for="(label, key) in {
          repoUrl: '仓库地址',
          token: '访问令牌',
          branch: '分支',
          directory: '图片目录',
        }"
        :key="key"
        class="field"
        ><span>{{ label }}</span
        ><input
          v-model="hosting[key]"
          class="input"
          :type="key === 'token' ? 'password' : 'text'"
          :placeholder="
            key === 'token' && hostingConfigured ? '已保存，留空保留' : ''
          "
      /></label>
      <div class="row">
        <button class="btn primary" :disabled="busy">保存</button
        ><button
          type="button"
          class="btn"
          :disabled="busy || !hostingConfigured"
          @click="testHosting"
        >
          测试连接
        </button>
      </div>
      <details class="detail-section">
        <summary>图片访问要求</summary>
        <p>发布图片需要可匿名读取的公开仓库。上传图片公开可见。</p>
      </details>
    </form>
    <form
      v-if="tab === 'pricing'"
      class="settings-form"
      @submit.prevent="action(() => api.saveSettings(settings))"
    >
      <h2>定价与文案</h2>
      <div class="form-grid">
        <label class="field"
          ><span>成本加价率（%）</span
          ><input
            v-model.number="settings.profitRate"
            class="input"
            type="number"
            min="10"
            max="20" /></label
        ><label class="field"
          ><span>最低加价（元）</span
          ><input
            v-model.number="settings.minProfit"
            class="input"
            type="number"
            min="0"
            step="0.01" /></label
        ><label class="field"
          ><span>默认品相</span
          ><input v-model="settings.defaultCondition" class="input" /></label
        ><label class="field"
          ><span>标题后缀</span
          ><input v-model="settings.titleSuffix" class="input" /></label
        ><label class="field wide"
          ><span>描述附加内容</span
          ><textarea v-model="settings.descFooter" class="input" rows="4" />
        </label>
      </div>
      <button class="btn primary" :disabled="busy">保存</button>
    </form>
    <form
      v-if="tab === 'app'"
      class="settings-form"
      @submit.prevent="action(() => api.saveSettings(settings))"
    >
      <h2>应用设置</h2>
      <label class="field"
        ><span>孔夫子搜索地址</span
        ><input v-model="settings.kongfzUrl" class="input" type="url" /></label
      ><label class="field"
        ><span>闲鱼网页发布地址</span
        ><input v-model="settings.publishUrl" class="input" type="url" /></label
      ><label class="check-line"
        ><input
          type="checkbox"
          v-model="settings.autoSubmit"
        />网页发布自动提交</label
      ><button class="btn primary" :disabled="busy">保存</button>
    </form>
  </section>
</template>
