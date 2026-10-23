import test from 'node:test';
import assert from 'node:assert/strict';
import { yearDifference } from '../src/commerce.js';

test('年度对比保留专业和原始版次，不将学校数解释为销量', () => {
  const make=(year,raw,major='数学')=>({data:{school:'甲大学',year,college:'理学院',major,stage:'初试',subject:'数学分析',raw}});
  const records=[make(2026,'数学分析 第2版 上册'),make(2027,'数学分析 第3版 上册'),make(2026,'高等代数'),make(2027,'高等代数'),make(2027,'高等代数','统计学')];
  const diff=yearDifference(records,'甲大学',2027,2026);
  assert.equal(diff.added.length,2);
  assert.equal(diff.removed.length,1);
  assert.equal(diff.removed[0].data.raw,'数学分析 第2版 上册');
});
