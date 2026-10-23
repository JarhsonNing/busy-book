use serde_json::json;

use crate::models::{suggested_price, Book, ImageItem, ListingInput, Settings};

pub struct BuiltListing {
    pub title: String,
    pub description: String,
    pub price: f64,
}

pub fn build(
    book: &Book,
    settings: &Settings,
    images: &[ImageItem],
    input: &ListingInput,
    base_url: &str,
) -> (BuiltListing, Vec<serde_json::Value>) {
    let price = input
        .price
        .or(book.sell_price)
        .or_else(|| {
            book.cost_price
                .map(|cost| suggested_price(cost, settings.profit_rate, settings.min_profit))
        })
        .unwrap_or(1.0)
        .max(0.01);

    let condition = input
        .condition
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&book.condition)
        .to_string();

    let base_title = book
        .title
        .trim()
        .trim_start_matches('《')
        .trim_end_matches('》')
        .to_string();
    let mut parts: Vec<String> = Vec::new();
    if !base_title.is_empty() {
        parts.push(format!("《{base_title}》"));
    }
    if !book.author.trim().is_empty() {
        parts.push(book.author.trim().to_string());
    }
    if !condition.is_empty() {
        parts.push(condition.clone());
    }
    if !settings.title_suffix.trim().is_empty() {
        parts.push(settings.title_suffix.trim().to_string());
    }
    let custom = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let title = custom
        .map(str::to_string)
        .unwrap_or_else(|| parts.join(" "))
        .chars()
        .take(30)
        .collect::<String>();

    let mut lines: Vec<String> = Vec::new();
    if !base_title.is_empty() {
        lines.push(format!("【书名】{base_title}"));
    }
    if !book.author.trim().is_empty() {
        lines.push(format!("【作者】{}", book.author.trim()));
    }
    if !book.publisher.trim().is_empty() {
        lines.push(format!("【出版社】{}", book.publisher.trim()));
    }
    if !book.edition.trim().is_empty() {
        lines.push(format!("【版次】{}", book.edition.trim()));
    }
    if !book.isbn.trim().is_empty() {
        lines.push(format!("【ISBN】{}", book.isbn.trim()));
    }
    if !condition.is_empty() {
        lines.push(format!("【品相】{condition}"));
    }
    if let Some(list_price) = book.list_price {
        lines.push(format!("【定价】{list_price:.2} 元"));
    }
    lines.push(format!("【售价】{price:.2} 元"));
    if !book.note.trim().is_empty() {
        lines.push(format!("【备注】{}", book.note.trim()));
    }

    let mut description = lines.join("\n");
    if !settings.desc_footer.trim().is_empty() {
        description.push_str("\n\n");
        description.push_str(settings.desc_footer.trim());
    }
    if let Some(custom) = input
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        description = custom.to_string();
    }

    let image_payload = images
        .iter()
        .enumerate()
        .map(|(index, image)| {
            let variant = if image.processed { "?variant=processed" } else { "" };
            json!({
                "id": image.id,
                "url": format!("{}/api/images/{}/file{}", base_url, image.id, variant),
                "name": format!("{}-{}.{}", book.id, index + 1, if image.processed { "png" } else { "jpg" }),
                "primary": image.is_primary,
            })
        })
        .collect();

    (
        BuiltListing {
            title,
            description,
            price,
        },
        image_payload,
    )
}

pub fn publish_url(base: &str, listing_id: i64) -> String {
    const DEFAULT_PUBLISH_URL: &str =
        "https://seller.goofish.com/?site=COMMONPRO#/seller-item/publish";
    let trimmed = base.trim();
    let base = if trimmed.is_empty() {
        DEFAULT_PUBLISH_URL
    } else {
        trimmed
    };
    // 任务参数必须放在 query 中。发布页使用 hash 路由，直接拼在末尾会把参数塞进 fragment。
    let (path, fragment) = match base.split_once('#') {
        Some((path, fragment)) => (path, format!("#{fragment}")),
        None => (base, String::new()),
    };
    let separator = if path.contains('?') { '&' } else { '?' };
    format!("{path}{separator}kf_listing={listing_id}{fragment}")
}
