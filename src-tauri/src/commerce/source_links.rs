use super::*;

// Link-only candidates acquire observed fields when the same product is collected.
// Never replace a user's checked quote, SKU, freight or other fulfillment conditions.
pub(super) fn enrich(c: &Connection, bid: i64, item: &Value) -> rusqlite::Result<bool> {
    let Some(url) = price_search::product_url(text(item,"url")) else { return Ok(false); };
    let existing = source_list(c, Some(bid))?;
    let Some(source) = existing.iter().find(|s| price_search::product_url(text(&s["data"],"url")).as_deref() == Some(&url)) else { return Ok(false); };
    let mut data = source["data"].clone();
    if data["origin"] == "manual-link" && data["status"] == "pending" && text(&data,"checkedAt").is_empty() {
        for key in ["title","shop","price"] {
            if data[key].is_null() || data[key] == "" { data[key] = item[key].clone(); }
        }
        c.execute("UPDATE book_sources SET data=?,updated_at=? WHERE id=?",params![data.to_string(),db::now(),source["id"].as_i64()])?;
    }
    Ok(true)
}

pub(super) async fn import(
    State(s): State<AppState>, Path(bid): Path<i64>, Json(input): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let urls = input["urls"].as_array().filter(|a| !a.is_empty() && a.len() <= 20)
        .ok_or_else(|| ApiError::bad("每次添加 1–20 个淘宝或天猫商品链接"))?;
    // Validate the whole batch before saving anything. Canonical URLs also remove tracking.
    let urls = urls.iter().map(|v| price_search::product_url(v.as_str().unwrap_or(""))
        .ok_or_else(|| ApiError::bad("请填写含商品 id 的淘宝或天猫完整链接；短链接请先在浏览器展开")))
        .collect::<Result<Vec<_>,_>>()?;
    let mut c = lock(&s);
    let tx = c.transaction()?;
    book_exists(&tx, bid)?;
    let existing = source_list(&tx, Some(bid))?;
    let mut known: std::collections::HashSet<String> = existing.iter()
        .filter_map(|s| price_search::product_url(text(&s["data"],"url"))).collect();
    let mut added = 0;
    for url in &urls {
        if !known.insert(url.clone()) { continue; }
        let data = json!({"url":url,"title":"","shop":"","sku":"","condition":"","price":null,"freight":null,
            "status":"pending","dropship":false,"checkedAt":"","origin":"manual-link"});
        tx.execute("INSERT INTO book_sources(book_id,data,updated_at) VALUES(?,?,?)",params![bid,data.to_string(),db::now()])?;
        added += 1;
    }
    event(&tx,"book",bid,"source-links",&json!({"added":added}))?;
    tx.commit()?;
    Ok(Json(json!({"added":added,"existing":urls.len()-added})))
}
