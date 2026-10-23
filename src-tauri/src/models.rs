use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    pub id: i64,
    pub item_id: String,
    pub url: String,
    pub title: String,
    pub price: Option<f64>,
    pub original_price: Option<f64>,
    pub shop: String,
    pub sales: String,
    pub main_image: String,
    pub images: Vec<String>,
    pub specs: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub id: i64,
    pub capture_id: Option<i64>,
    pub item_id: String,
    pub title: String,
    pub author: String,
    pub publisher: String,
    pub isbn: String,
    pub edition: String,
    pub condition: String,
    pub cost_price: Option<f64>,
    pub list_price: Option<f64>,
    pub sell_price: Option<f64>,
    pub stock: i64,
    pub status: String,
    pub note: String,
    pub image_count: i64,
    pub cover_url: String,
    pub listing_id: Option<i64>,
    pub listing_status: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ImageRow {
    pub id: i64,
    pub book_id: i64,
    pub source: String,
    pub origin_url: String,
    pub origin_file: String,
    pub processed_file: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub is_primary: i64,
    pub sort: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageItem {
    pub id: i64,
    pub book_id: i64,
    pub source: String,
    pub origin_url: String,
    pub has_origin: bool,
    pub processed: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub is_primary: bool,
    pub sort: i64,
    pub url: String,
    pub processed_url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub id: i64,
    pub book_id: i64,
    pub platform: String,
    pub title: String,
    pub description: String,
    pub price: Option<f64>,
    pub status: String,
    pub url: String,
    pub error: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookDetail {
    #[serde(flatten)]
    pub book: Book,
    pub capture: Option<Capture>,
    pub images: Vec<ImageItem>,
    pub listings: Vec<Listing>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInput {
    #[serde(default)]
    pub as_source: bool,
    pub item_id: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub price: Option<Value>,
    pub original_price: Option<Value>,
    pub shop: Option<String>,
    pub sales: Option<String>,
    pub main_image: Option<String>,
    pub images: Option<Vec<String>>,
    pub specs: Option<Value>,
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookPatch {
    pub title: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub isbn: Option<String>,
    pub edition: Option<String>,
    pub condition: Option<String>,
    pub cost_price: Option<f64>,
    pub list_price: Option<f64>,
    pub sell_price: Option<f64>,
    pub stock: Option<i64>,
    pub status: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageImportInput {
    pub urls: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub price: Option<f64>,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingReport {
    pub status: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Cost markup percentage, e.g. 15 means cost * 1.15.
    pub profit_rate: f64,
    /// Minimum absolute profit in yuan, e.g. 5 means at least cost + 5.
    pub min_profit: f64,
    pub default_condition: String,
    pub title_suffix: String,
    pub desc_footer: String,
    pub auto_submit: bool,
    pub publish_url: String,
    pub kongfz_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profit_rate: 15.0,
            min_profit: 5.0,
            default_condition: "九成新".into(),
            title_suffix: "二手书".into(),
            desc_footer: "实物拍摄，品相如图。二手书难免有轻微使用痕迹，介意慎拍。".into(),
            auto_submit: true,
            publish_url: "https://seller.goofish.com/?site=COMMONPRO#/seller-item/publish".into(),
            kongfz_url: "https://search.kongfz.com/product_result/".into(),
        }
    }
}

/// Suggested selling price: cost plus the configured markup, with a minimum
/// absolute profit floor, rounded up to a whole yuan.
pub fn suggested_price(cost: f64, profit_rate: f64, min_profit: f64) -> f64 {
    if !cost.is_finite() || cost <= 0.0 {
        return 1.0;
    }
    let rate = profit_rate.clamp(0.0, 100.0) / 100.0;
    let floor = min_profit.max(0.0);
    ceil_yuan((cost * (1.0 + rate)).max(cost + floor)).max(1.0)
}

fn ceil_yuan(value: f64) -> f64 {
    // Avoid over-rounding 110.00000000000001 to 111 due to binary floats.
    let nearest = value.round();
    if (value - nearest).abs() < 1e-9 {
        nearest
    } else {
        value.ceil()
    }
}

#[derive(Debug, Clone)]
pub struct ImportContext {
    pub book_id: i64,
    pub title: String,
    pub isbn: String,
    pub created: Instant,
}

#[cfg(test)]
mod tests {
    use super::suggested_price;

    #[test]
    fn pricing_respects_rate_range_and_profit_floor() {
        assert_eq!(suggested_price(100.0, 10.0, 5.0), 110.0);
        assert_eq!(suggested_price(100.0, 15.0, 5.0), 115.0);
        assert_eq!(suggested_price(100.0, 20.0, 5.0), 120.0);
        assert_eq!(suggested_price(12.5, 15.0, 5.0), 18.0);
        assert_eq!(suggested_price(1.68, 15.0, 5.0), 7.0);
    }
}
