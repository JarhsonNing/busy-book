use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Regions {
    provinces: Vec<Province>,
}
#[derive(Deserialize)]
struct Province {
    id: i64,
    cities: Vec<City>,
}
#[derive(Deserialize)]
struct City {
    id: i64,
    districts: Vec<District>,
}
#[derive(Deserialize)]
struct District {
    id: i64,
}

pub fn valid(province: i64, city: i64, district: i64) -> bool {
    static DATA: OnceLock<Regions> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/xianyu-regions.json"))
            .expect("validated region data")
    })
    .provinces
    .iter()
    .find(|p| p.id == province)
    .and_then(|p| p.cities.iter().find(|c| c.id == city))
    .map(|c| c.districts.iter().any(|d| d.id == district))
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::valid;
    #[test]
    fn validates_municipalities_and_actual_parent_relationships() {
        assert!(valid(110000, 110100, 110101));
        assert!(valid(440000, 440300, 440305));
        assert!(!valid(110000, 440300, 440305));
        assert!(!valid(440000, 440300, 110101));
        assert!(!valid(0, 0, 0));
    }
}
