export const productStates = {
  '-1': '已删除',
  21: '待发布',
  22: '销售中',
  23: '已售罄',
  31: '手动下架',
  33: '售出下架',
  36: '自动下架',
};
export const orderStates = {
  11: '待付款',
  12: '待发货',
  21: '已发货',
  22: '已完成',
  23: '已退款',
  24: '已关闭',
};
export const taskStates = {
  preparing: '创建中',
  created: '已创建 / 待上架',
  publishing: '上架处理中',
  published: '已上架',
  offline: '已下架',
  failed: '操作失败',
  unknown: '结果未知 / 需核查',
  base64_rejected: '图片被拒绝',
  deleted: '远端已删除',
};
export const imageModes = {
  gitee: 'Gitee 自动上传',
  'data-url': 'Base64 Data URL',
  'raw-base64': '纯 Base64',
  url: '公网 URL',
};
export const yuan = (cents) => (cents == null ? '—' : '¥' + (Number(cents) / 100).toFixed(2));
export const date = (seconds) =>
  seconds ? new Date(Number(seconds) * 1000).toLocaleString('zh-CN') : '—';
export const cents = (yuan) => Math.round(Number(yuan) * 100);
export const listOf = (data) => (Array.isArray(data) ? data : data?.list || []);
export const itemUrl = (id) => 'https://www.goofish.com/item?id=' + encodeURIComponent(id);
export function weightedLength(value) {
  return Array.from(value || '').reduce((n, c) => n + (c.codePointAt(0) > 127 ? 2 : 1), 0);
}

export const normalizeIsbn = (value) =>
  String(value || '')
    .replace(/[\s-]/g, '')
    .toUpperCase();
export const validIsbn = (value) =>
  /^(?:[0-9]{9}[0-9X]|97[89][0-9]{10})$/.test(normalizeIsbn(value));
