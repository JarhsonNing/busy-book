import assert from 'node:assert/strict';
import test from 'node:test';
import {
  provinces,
  citiesFor,
  districtsFor,
  isValidRegion,
  selectProvince,
  selectCity,
} from '../src/regions.js';

test('supplied region table includes municipalities and special administrative regions', () => {
  assert.equal(provinces.length, 34);
  assert.equal(provinces.flatMap((p) => p.cities).length, 372);
  assert.equal(provinces.flatMap((p) => p.cities.flatMap((c) => c.districts)).length, 3639);
  assert.equal(citiesFor(110000)[0].name, '北京市');
  assert.equal(districtsFor(110000, 110100).find((d) => d.id === 110101).name, '东城区');
  assert.equal(provinces.find((p) => p.id === 820000).name, '澳门特别行政区');
});
test('changing parent selections clears stale children', () => {
  assert.deepEqual(selectProvince('440000'), { province: 440000, city: '', district: '' });
  assert.deepEqual(selectCity(440000, '440300'), { province: 440000, city: 440300, district: '' });
  assert.deepEqual(selectProvince(''), { province: '', city: '', district: '' });
});
test('saved addresses accept numeric strings but require matching parents', () => {
  assert.ok(isValidRegion({ province: '440000', city: '440300', district: '440305' }));
  assert.ok(!isValidRegion({ province: 110000, city: 440300, district: 440305 }));
  assert.ok(!isValidRegion({ province: 440000, city: 440300, district: '' }));
  assert.deepEqual(citiesFor(''), []);
  assert.deepEqual(districtsFor(440000, 110100), []);
});
