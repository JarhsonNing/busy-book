use super::*;
use std::collections::HashSet;

const BATCH_LIMIT: u64 = 3;
fn read(c: &Connection, bid: i64) -> Result<Value, ApiError> {
    book_exists(c, bid)?;
    let raw: Option<String> = c
        .query_row(
            "SELECT data FROM price_searches WHERE book_id=?",
            [bid],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.map(parse).unwrap_or(
        json!({"bookId":bid,"state":"idle","pagesDone":0,"maxPages":BATCH_LIMIT,"items":[]}),
    ))
}
fn write(c: &Connection, bid: i64, v: &Value) -> Result<(), ApiError> {
    c.execute("INSERT INTO price_searches(book_id,data) VALUES(?,?) ON CONFLICT(book_id) DO UPDATE SET data=excluded.data",params![bid,v.to_string()])?;
    Ok(())
}
fn url(v: &Value, bid: i64) -> String {
    let mut u = reqwest::Url::parse("https://s.taobao.com/search").unwrap();
    u.query_pairs_mut()
        .append_pair("q", text(v, "query"))
        .append_pair("sort", "price-asc")
        .append_pair("taoshuSourceBook", &bid.to_string())
        .append_pair("taoshuPriceSearch", text(v, "requestId"));
    u.into()
}
pub(crate) fn product_url(raw: &str) -> Option<String> {
    let u = reqwest::Url::parse(raw).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some()
    {
        return None;
    }
    let host = u.host_str()?;
    if !["item.taobao.com", "detail.tmall.com"].contains(&host) {
        return None;
    }
    let id = u.query_pairs().find(|(k, _)| k == "id")?.1.into_owned();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("https://{host}/item.htm?id={id}"))
}
fn money(v: &Value) -> Option<i64> {
    v.as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 1_000_000.0)
        .map(|n| (n * 100.0).round() as i64)
}
fn clean(v: &Value, key: &str) -> String {
    text(v, key).trim().chars().take(600).collect()
}
fn normalize(items: &Value) -> Vec<Value> {
    let mut seen = HashSet::new();
    items.as_array().into_iter().flatten().take(60).filter_map(|i| {
        let u = product_url(text(i,"url"))?;
        let title = clean(i,"title");
        if title.is_empty() || !seen.insert(u.clone()) { return None; }
        Some(json!({"url":u,"title":title,"shop":clean(i,"shop"),"priceLabel":clean(i,"priceLabel"),"price":money(&i["price"]).map(|n|n as f64/100.0)}))
    }).collect()
}
fn decorate(c: &Connection, bid: i64, mut v: Value) -> Result<Value, ApiError> {
    let book = db::get_book(c, bid)?.ok_or_else(|| ApiError::bad("图书不存在"))?;
    let observed = v["items"].as_array().cloned().unwrap_or_default();
    let sources = source_list(c, Some(bid))?;
    let mut rows = Vec::new();
    for source in sources {
        let d = &source["data"];
        let Some(u) = product_url(text(d, "url")) else {
            continue;
        };
        let snapshot = observed.iter().find(|i| i["url"] == u);
        let price = money(&d["price"]);
        let freight = money(&d["freight"]);
        let checked = d["status"] == "available"
            && !text(d, "sku").is_empty()
            && !text(d, "checkedAt").is_empty();
        let condition_checked = !book.condition.trim().is_empty()
            && text(d, "condition").trim() == book.condition.trim();
        let total = if checked && condition_checked && d["dropship"] == true {
            price.zip(freight).map(|(a, b)| a + b)
        } else {
            None
        };
        let reason = if d["status"] == "unavailable" {
            "不可用"
        } else if !checked {
            "待核验版本与规格"
        } else if book.condition.trim().is_empty() {
            "图书品相待设置"
        } else if text(d, "condition").trim().is_empty() {
            "品相待核"
        } else if !condition_checked {
            "品相不符"
        } else if d["dropship"] != true {
            "代发待确认"
        } else if freight.is_none() {
            "运费待核"
        } else if price.is_none() {
            "采购价待核"
        } else {
            "已核验"
        };
        let title = snapshot
            .map(|s| s["title"].clone())
            .unwrap_or(d["title"].clone());
        rows.push(json!({"source":source,"title":title,
            "displayPrice":snapshot.and_then(|s|money(&s["price"])).map(|n|n as f64/100.0),
            "priceLabel":snapshot.map(|s|s["priceLabel"].clone()).unwrap_or(Value::Null),
            "total":total.map(|n|n as f64/100.0),"reason":reason,"observed":snapshot.is_some()}));
    }
    rows.sort_by_key(|r| {
        (
            money(&r["total"]).is_none(),
            money(&r["total"])
                .or_else(|| money(&r["displayPrice"]))
                .unwrap_or(i64::MAX),
            r["source"]["id"].as_i64(),
        )
    });
    v["bestVerified"] = rows
        .iter()
        .find(|r| !r["total"].is_null())
        .cloned()
        .unwrap_or(Value::Null);
    v["bestDisplay"] = rows
        .iter()
        .filter(|r| !r["displayPrice"].is_null())
        .min_by_key(|r| money(&r["displayPrice"]).unwrap())
        .cloned()
        .unwrap_or(Value::Null);
    v["rows"] = json!(rows);
    v["count"] = json!(observed.len());
    // Keep the stored page counters for compatibility with existing task snapshots.
    v["batchesDone"] = v["pagesDone"].clone();
    v["maxItems"] = json!(BATCH_LIMIT * 60);
    v["mode"] = json!("scroll");
    if ["searching", "waiting"].contains(&text(&v, "state")) {
        v["searchUrl"] = json!(url(&v, bid));
    }
    Ok(v)
}
pub(super) async fn status(
    State(s): State<AppState>,
    Path(bid): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let mut v = read(&c, bid)?;
    if v["state"] == "searching"
        && chrono::Utc::now().timestamp_millis() - v["touchedAt"].as_i64().unwrap_or(0) > 180000
    {
        v["state"] = json!("waiting");
        v["error"] = json!("未收到网站结果，请登录或完成页面验证后继续");
        write(&c, bid, &v)?;
    }
    Ok(Json(decorate(&c, bid, v)?))
}
pub(super) async fn control(
    State(s): State<AppState>,
    Path(bid): Path<i64>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let ai_request = if input["action"] == "start" && !input["ai"].is_null() {
        if !input["ai"].is_object() { return Err(ApiError::bad("AI 整理配置无效")); }
        Some(crate::agents::ai::source_search_request(&s, &input["ai"])?)
    } else { None };
    let mut c = lock(&s);
    let tx = c.transaction()?;
    let mut v = read(&tx, bid)?;
    let action = required(&input, "action")?;
    if action == "start" {
        let b = db::get_book(&tx, bid)?.unwrap();
        let query = if !b.isbn.trim().is_empty() {
            b.isbn
        } else {
            [b.title, b.author, b.edition]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        };
        if query.trim().is_empty() {
            return Err(ApiError::bad("请先填写书名或 ISBN"));
        }
        v = json!({"bookId":bid,"requestId":format!("{}-{}",chrono::Utc::now().timestamp_millis(),rand::random::<u64>()),"query":query,
            "state":"searching","maxPages":BATCH_LIMIT,"pagesDone":0,"items":[],"error":"","startedAt":db::now()});
        if let Some(req) = ai_request { v["aiRequest"] = req; }
    } else {
        if text(&v, "requestId").is_empty() || input["requestId"] != v["requestId"] {
            return Err(ApiError::bad("比价任务已过期，请刷新"));
        }
        let batch = if action == "batch" {
            &input["batch"]
        } else {
            &input["page"]
        };
        if (action == "page" || action == "batch")
            && batch
                .as_u64()
                .is_some_and(|p| p > 0 && p <= v["pagesDone"].as_u64().unwrap_or(0))
        {
            return Ok(Json(decorate(&tx, bid, v)?));
        }
        if action == "finish" && v["state"] == "completed" {
            return Ok(Json(decorate(&tx, bid, v)?));
        }
        if !["searching", "waiting"].contains(&text(&v, "state")) {
            return Err(ApiError::bad("比价任务已经结束"));
        }
        if ["page", "batch", "finish"].contains(&action.as_str()) && v["state"] != "searching" {
            return Err(ApiError::bad("比价已暂停，请继续检索后再采集"));
        }
        match action.as_str() {
            "resume" => {
                v["state"] = json!("searching");
                v["error"] = json!("");
            }
            "cancel" => {
                v["state"] = json!("canceled");
                v["error"] = json!("");
            }
            "waiting" => {
                v["state"] = json!("waiting");
                v["error"] = json!(clean(&input, "error"));
            }
            "finish" => {
                if input["reason"] != "visible-end"
                    || v["items"].as_array().is_none_or(|a| a.is_empty())
                {
                    return Err(ApiError::bad("未读取到有效商品，请检查淘宝页面后继续"));
                }
                v["state"] = json!("completed");
                v["completionReason"] = json!("visible-end");
                v["error"] = json!("");
            }
            "page" | "batch" => {
                let page = batch
                    .as_u64()
                    .ok_or_else(|| ApiError::bad("采集批次无效"))?;
                let done = v["pagesDone"].as_u64().unwrap_or(0);
                if page == 0 || page > BATCH_LIMIT || page > done + 1 {
                    return Err(ApiError::bad("采集批次顺序错误"));
                }
                if page <= done {
                    return Ok(Json(decorate(&tx, bid, v)?));
                }
                let items = normalize(&input["items"]);
                if items.is_empty() {
                    return Err(ApiError::bad("未读取到有效商品，请检查网站登录或页面状态"));
                }
                let request_id = v["requestId"].clone();
                let mut known_urls: HashSet<String> = source_list(&tx, Some(bid))?
                    .iter()
                    .filter_map(|s| product_url(text(&s["data"], "url")))
                    .collect();
                let previous = v["items"].as_array_mut().unwrap();
                let mut added = 0;
                for item in items {
                    if previous.iter().any(|i| i["url"] == item["url"]) {
                        continue;
                    }
                    source_links::enrich(&tx, bid, &item)?;
                    if known_urls.insert(text(&item, "url").to_string()) {
                        let capture_input: crate::models::CaptureInput = serde_json::from_value(
                            json!({"asSource":true,"sourceBookId":bid,
                        "url":item["url"],"itemId":reqwest::Url::parse(text(&item,"url")).unwrap().query_pairs().find(|(k,_)|k=="id").unwrap().1,
                        "title":item["title"],"price":item["price"],"shop":item["shop"],"raw":{"source":"price-search","requestId":request_id,"batch":page,"priceLabel":item["priceLabel"]}}),
                        )?;
                        let (capture, _, _) =
                            db::upsert_capture(&tx, &capture_input, 0.3, 5.0, "九成新")?;
                        capture_candidate(&tx, &capture)?;
                        attach_capture_candidate(&tx, &capture, bid)?;
                    }
                    previous.push(item);
                    added += 1;
                }
                if added == 0 {
                    v["state"] = json!("waiting");
                    v["error"] = json!("未读取到新商品，已保留候选；请检查淘宝加载状态后继续");
                } else {
                    v["pagesDone"] = json!(page);
                    v["state"] = json!(if page >= BATCH_LIMIT {
                        "completed"
                    } else {
                        "searching"
                    });
                    if page >= BATCH_LIMIT {
                        v["completionReason"] = json!("batch-limit");
                    }
                    v["error"] = json!("");
                }
            }
            _ => return Err(ApiError::bad("未知比价操作")),
        }
    }
    if action == "start" || action == "resume" {
        // The desktop reuses a single Taobao window, so another book cannot keep scanning.
        tx.execute("UPDATE price_searches SET data=json_set(data,'$.state','waiting','$.error','已切换到另一图书的比价，可稍后继续') WHERE book_id<>? AND json_extract(data,'$.state')='searching'",[bid])?;
    }
    v["updatedAt"] = json!(db::now());
    v["touchedAt"] = json!(chrono::Utc::now().timestamp_millis());
    write(&tx, bid, &v)?;
    if v["aiRequest"].is_object() {
        // Queue once, including an explicit stop with partial results. Persist the frozen
        // executor/model/prompt so leaving the page or restarting cannot change them.
        if (v["state"] == "completed" || (v["state"] == "canceled" && input["analyze"] == true))
            && v["analysisJobId"].is_null() && !source_list(&tx, Some(bid))?.is_empty() {
            match crate::agents::ai::enqueue_prepared(&tx, &json!({"bookIds":[bid]}), v["aiRequest"].clone()) {
                Ok(job) => { v["analysisJobId"] = job["id"].clone(); v["analysisError"] = json!(""); }
                Err(error) => { v["analysisError"] = json!(error.message); }
            }
            write(&tx, bid, &v)?;
        }
        crate::agents::ai::refresh_reference_cost(&tx,bid)?;
    } else if ["page","batch","finish"].contains(&action.as_str()) {
        crate::agents::ai::auto_queue(&tx,bid,"source")?;
    }
    let result = decorate(&tx, bid, v)?;
    tx.commit()?;
    Ok(Json(result))
}
