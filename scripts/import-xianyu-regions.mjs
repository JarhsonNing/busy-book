import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const source = path.resolve(
  process.argv[2] || path.join(root, 'resources/闲管家省市区-20261019.sql'),
);
const version = path.basename(source).match(/(\d{8})\.sql$/)?.[1];
if (!version) throw new Error('SQL 文件名必须包含 YYYYMMDD 版本日期');
const sql = (await fs.readFile(source, 'utf8')).replace(/^\uFEFF/, '');
const provinces = new Map();
const districtIds = new Set();
const rowPattern =
  /^INSERT INTO `xgj_region` \(`prov_id`, `prov_name`, `city_id`, `city_name`, `area_id`, `area_name`\) VALUES \((\d+), '((?:[^']|'')*)', (\d+), '((?:[^']|'')*)', (\d+), '((?:[^']|'')*)'\);$/;

// Parse data only. Never execute statements from the supplied SQL file.
for (const [index, line] of sql.split(/\r?\n/).entries()) {
  if (!line.trim()) continue;
  const match = rowPattern.exec(line.trim());
  if (!match) throw new Error(`第 ${index + 1} 行不符合地区数据格式`);
  const [, provinceId, provinceText, cityId, cityText, districtId, districtText] = match;
  const [province, city, district] = [provinceId, cityId, districtId].map(Number);
  const [provinceName, cityName, districtName] = [provinceText, cityText, districtText].map((s) =>
    s.replaceAll("''", "'"),
  );
  if (districtIds.has(district)) throw new Error(`区县编码重复：${district}`);
  districtIds.add(district);
  if (!provinces.has(province))
    provinces.set(province, { id: province, name: provinceName, cities: new Map() });
  const parent = provinces.get(province);
  if (parent.name !== provinceName) throw new Error(`省份名称冲突：${province}`);
  if (!parent.cities.has(city))
    parent.cities.set(city, { id: city, name: cityName, districts: [] });
  const cityNode = parent.cities.get(city);
  if (cityNode.name !== cityName) throw new Error(`城市名称冲突：${city}`);
  cityNode.districts.push({ id: district, name: districtName });
}
if (!districtIds.size) throw new Error('SQL 中没有地区数据');
const data = {
  version,
  provinces: [...provinces.values()].map((p) => ({ ...p, cities: [...p.cities.values()] })),
};
const destination = path.join(root, 'src-tauri/resources/xianyu-regions.json');
await fs.mkdir(path.dirname(destination), { recursive: true });
await fs.writeFile(destination, JSON.stringify(data) + '\n');
console.log(
  `地区数据 ${version}：${data.provinces.length} 省、${data.provinces.reduce((sum, p) => sum + p.cities.length, 0)} 市、${districtIds.size} 区县`,
);
