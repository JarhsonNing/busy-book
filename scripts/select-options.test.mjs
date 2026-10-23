import test from 'node:test';
import assert from 'node:assert/strict';
import { h, Fragment } from 'vue';
import { collectOptions, filterOptions } from '../src/components/ui/select-options.js';
test('可搜索下拉保留 null、空字符串、数值和大整数远端 ID', () => {
  const options = collectOptions([
    h('option', { value: null }, '未选择'),
    h(Fragment, null, [
      h('option', { value: '' }, '全部'),
      h('option', { value: 2027 }, '2027 年'),
      h('option', { value: '9007199254740997' }, '图书 A'),
      h('option', { disabled: true }, '不可用'),
    ]),
  ]);
  assert.deepEqual(
    options.map((o) => o.value),
    [null, '', 2027, '9007199254740997', '不可用'],
  );
  assert.equal(options[4].disabled, true);
  assert.equal(filterOptions(options, '9007199254740997')[0].value, '9007199254740997');
  assert.equal(filterOptions(options, '图书 a').length, 1);
  assert.equal(filterOptions(options, '不存在').length, 0);
});
