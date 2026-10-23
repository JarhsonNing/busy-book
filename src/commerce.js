export const fulfillmentStates = {
  unmatched: '待确认图书与规格',
  waiting_payment: '待付款',
  to_purchase: '实体版待采购',
  awaiting_tracking: '已采购待物流',
  purchased: '已采购',
  ready_to_ship: '待回传物流',
  shipping_unknown: '发货结果待核查',
  shipped: '已发货',
  completed: '已完成',
  delivery_pending: '电子交付待核查',
  delivered: '电子版已交付',
  after_sale: '售后待处理',
  canceled_review: '订单取消待核查采购',
  closed: '已结案',
};
export const kindName = (kind) =>
  ({ physical: '实体版', digital: '电子版', unknown: '待确认规格' })[kind] || kind;
export const showTime = (value) => (value ? new Date(value).toLocaleString('zh-CN') : '未核验');
export const emptySource = () => ({
  url: '',
  title: '',
  shop: '',
  sku: '',
  condition: '九成新',
  price: null,
  freight: null,
  dropship: false,
  status: 'pending',
  checkedAt: '',
});
export function yearDifference(records, school, year, priorYear) {
  const identity = (r) =>
    JSON.stringify([
      r.data.college,
      r.data.majorCode,
      r.data.major,
      r.data.stage,
      r.data.subjectCode,
      r.data.subject,
      r.data.raw,
    ]);
  const current = records.filter(
    (r) => r.data.school === school && Number(r.data.year) === Number(year),
  );
  const prior = records.filter(
    (r) => r.data.school === school && Number(r.data.year) === Number(priorYear),
  );
  const a = new Set(current.map(identity)),
    b = new Set(prior.map(identity));
  return {
    added: current.filter((r) => !b.has(identity(r))),
    removed: prior.filter((r) => !a.has(identity(r))),
  };
}
