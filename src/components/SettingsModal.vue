<script setup>
import { reactive } from 'vue';

const props = defineProps({
  settings: { type: Object, default: () => ({}) },
});
const emit = defineEmits(['close', 'save']);

const form = reactive({
  profitRate: props.settings.profitRate ?? 15,
  minProfit: props.settings.minProfit ?? 5,
  defaultCondition: props.settings.defaultCondition ?? '九成新',
  titleSuffix: props.settings.titleSuffix ?? '二手书',
  descFooter: props.settings.descFooter ?? '',
  autoSubmit: props.settings.autoSubmit ?? true,
  publishUrl: props.settings.publishUrl ?? 'https://seller.goofish.com/?site=COMMONPRO#/seller-item/publish',
  kongfzUrl: props.settings.kongfzUrl ?? 'https://search.kongfz.com/product_result/',
});

function submit() {
  const profitRate = Number(form.profitRate);
  const minProfit = Number(form.minProfit);
  emit('save', {
    ...form,
    profitRate: Number.isFinite(profitRate) ? Math.min(20, Math.max(10, profitRate)) : 15,
    minProfit: Number.isFinite(minProfit) ? Math.max(0, minProfit) : 5,
  });
}
</script>

<template>
  <div class="modal-backdrop" @click.self="$emit('close')">
    <div class="modal settings-modal">
      <header class="modal-head">
        <div>
          <h2>设置</h2>
          <p>定价策略、发布文案和窗口地址</p>
        </div>
        <button class="icon-btn" @click="$emit('close')">×</button>
      </header>

      <div class="modal-body">
        <div class="form-grid two">
          <label class="field">
            <span>目标利润率（%）</span>
            <input v-model="form.profitRate" type="number" step="1" min="10" max="20" class="input" />
            <small>按成本加成计算，建议 10%～20%；例如成本 ¥10、15% → ¥11.50</small>
          </label>
          <label class="field">
            <span>最低利润（元）</span>
            <input v-model="form.minProfit" type="number" step="1" min="0" class="input" />
            <small>售价不低于「成本 + 最低利润」；默认 5 元，向上取整到元</small>
          </label>
        </div>

        <label class="field">
          <span>默认品相</span>
          <input v-model="form.defaultCondition" class="input" placeholder="九成新" />
        </label>

        <label class="field">
          <span>标题后缀</span>
          <input v-model="form.titleSuffix" class="input" placeholder="二手书" />
          <small>生成的闲鱼标题会拼上「书名 + 作者 + 品相 + 后缀」，最长 30 字</small>
        </label>

        <label class="field">
          <span>描述页脚</span>
          <textarea v-model="form.descFooter" class="input textarea" rows="3" />
        </label>

        <div class="form-grid two">
          <label class="field">
            <span>闲鱼发布地址</span>
            <input v-model="form.publishUrl" class="input" />
            <small>建议使用商品发布页；脚本也兼容卖家中心首页，会自动查找「发闲置」入口</small>
          </label>
          <label class="field">
            <span>孔夫子搜索地址</span>
            <input v-model="form.kongfzUrl" class="input" />
            <small>找图时会追加 ?key=ISBN 或书名</small>
          </label>
        </div>

        <label class="switch">
          <input v-model="form.autoSubmit" type="checkbox" />
          <span class="switch-track"></span>
          <span class="switch-text">
            自动提交发布
            <small>填完表后自动点击「发布」。关闭后只填表，由你确认提交。</small>
          </span>
        </label>
      </div>

      <footer class="modal-foot">
        <button class="btn ghost" @click="$emit('close')">取消</button>
        <button class="btn accent" @click="submit">保存设置</button>
      </footer>
    </div>
  </div>
</template>
