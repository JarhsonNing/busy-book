import data from '../src-tauri/resources/xianyu-regions.json' with { type: 'json' };

export const regionVersion = data.version;
export const provinces = data.provinces;
export const citiesFor = (province) =>
  provinces.find((p) => p.id === Number(province))?.cities || [];
export const districtsFor = (province, city) =>
  citiesFor(province).find((c) => c.id === Number(city))?.districts || [];
export const isValidRegion = ({ province, city, district }) =>
  districtsFor(province, city).some((d) => d.id === Number(district));
export const selectProvince = (province) => ({
  province: province ? Number(province) : '',
  city: '',
  district: '',
});
export const selectCity = (province, city) => ({
  province: Number(province),
  city: city ? Number(city) : '',
  district: '',
});
