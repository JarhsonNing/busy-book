//! Book-centered operations. Platform identifiers are stored as text, snapshots never contain credentials.
mod catalog;
mod catalog_browser;
mod catalog_collect;
mod fulfillment;
pub mod jobs;
mod library;
mod book_search;
pub(crate) mod price_search;
pub mod schema;
#[cfg(test)]
mod tests;
use crate::{
    api::ApiError,
    db,
    state::{lock, AppState},
};
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
pub use fulfillment::{observe_order, record_delivery, refund_blocks};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/commerce/overview", get(overview))
        .route("/api/commerce/library", get(library::list))
        .route("/api/commerce/catalog/sync", post(library::sync_catalog))
        .route("/api/commerce/jobs", get(jobs::list).post(jobs::create))
        .route("/api/commerce/jobs/{id}", get(jobs::detail))
        .route("/api/commerce/jobs/{id}/{action}", post(jobs::control))
        .route("/api/commerce/books", get(books))
        .route("/api/commerce/books/{id}", get(book_detail))
        .route("/api/commerce/books/{id}/price-search", get(price_search::status).post(price_search::control))
        .route("/api/commerce/books/{id}/profile", post(save_profile))
        .route("/api/commerce/sources", get(sources).post(save_source))
        .route("/api/commerce/books/{id}/resources", post(add_resource))
        .route("/api/commerce/resources/{id}/state", post(resource_state))
        .route("/api/commerce/channels", post(save_channel))
        .route("/api/commerce/channels/{id}/check", get(channel_check))
        .route("/api/commerce/catalog/preview", post(catalog::preview))
        .route("/api/commerce/catalog/import", post(catalog::import))
        .route("/api/commerce/catalog/status", get(catalog_browser::status))
        .route(
            "/api/commerce/catalog/path",
            post(catalog_browser::set_path),
        )
        .route("/api/commerce/catalog/tree", get(catalog_browser::tree))
        .route("/api/commerce/catalog/books", get(catalog_browser::books))
        .route(
            "/api/commerce/catalog/major/{programId}/books",
            get(catalog_browser::major_books),
        )
        .route(
            "/api/commerce/catalog/book/{id}/usage",
            get(catalog_browser::book_usage),
        )
        .route(
            "/api/commerce/catalog/adapters",
            get(catalog_collect::adapters).post(catalog_collect::save_adapter),
        )
        .route(
            "/api/commerce/catalog/delete-adapter",
            post(catalog_collect::delete_adapter),
        )
        .route(
            "/api/commerce/catalog/collect",
            post(catalog_collect::collect),
        )
        .route("/api/commerce/requirements", get(catalog::list))
        .route("/api/commerce/requirements/{id}/search", get(book_search::status).post(book_search::update))
        .route(
            "/api/commerce/requirements/{id}/confirm",
            post(catalog::confirm),
        )
        .route("/api/commerce/fulfillments", get(fulfillment::list))
        .route("/api/commerce/fulfillments/{id}", get(fulfillment::detail))
        .route("/api/commerce/fulfillments/{id}/tracking", post(fulfillment::tracking))
        .route(
            "/api/commerce/fulfillments/{id}/purchase",
            post(fulfillment::purchase),
        )
        .route(
            "/api/commerce/fulfillments/{id}/finance",
            post(fulfillment::finance),
        )
        .route(
            "/api/commerce/fulfillments/{id}/note",
            post(fulfillment::note),
        )
}
pub fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("").trim()
}
pub fn remote_id(v: &Value) -> String {
    v.as_str()
        .map(str::to_string)
        .or_else(|| v.as_u64().map(|n| n.to_string()))
        .unwrap_or_default()
}
fn required(v: &Value, key: &str) -> Result<String, ApiError> {
    let s = text(v, key);
    if s.is_empty() {
        Err(ApiError::bad(format!("请填写 {key}")))
    } else {
        Ok(s.to_string())
    }
}
pub fn app_key(conn: &Connection) -> String {
    db::get_setting(conn, "xianyuAppKey")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}
pub fn parse(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(json!({}))
}
pub fn rows(
    conn: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> rusqlite::Result<Vec<Value>> {
    conn.prepare(sql)?
        .query_map(args, |r| Ok(parse(r.get(0)?)))?
        .collect()
}
pub fn event(
    conn: &Connection,
    entity: &str,
    id: impl ToString,
    action: &str,
    data: &Value,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO commerce_events(entity,entity_id,action,data,created_at) VALUES(?,?,?,?,?)",
        params![entity, id.to_string(), action, data.to_string(), db::now()],
    )?;
    Ok(())
}
fn book_exists(conn: &Connection, id: i64) -> Result<(), ApiError> {
    if db::get_book(conn, id)?.is_none() {
        Err(ApiError::bad("图书不存在"))
    } else {
        Ok(())
    }
}
fn amount(v: &Value, key: &str, optional: bool) -> Result<(), ApiError> {
    if optional && v[key].is_null() {
        return Ok(());
    }
    let n = v[key]
        .as_f64()
        .ok_or_else(|| ApiError::bad(format!("{key} 必须为金额")))?;
    if !n.is_finite() || n < 0.0 || n > 1000000.0 {
        Err(ApiError::bad("金额超出范围"))
    } else {
        Ok(())
    }
}
fn web_url(s: &str) -> Result<(), ApiError> {
    let u = reqwest::Url::parse(s).map_err(|_| ApiError::bad("链接格式错误"))?;
    if !["http", "https"].contains(&u.scheme())
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err(ApiError::bad("请填写不含登录凭据的 HTTP/HTTPS 链接"));
    }
    Ok(())
}

pub fn source_list(conn: &Connection, book: Option<i64>) -> rusqlite::Result<Vec<Value>> {
    rows(conn,"SELECT json_object('id',id,'bookId',book_id,'captureId',capture_id,'primary',json(CASE WHEN is_primary=1 THEN 'true' ELSE 'false' END),'data',json(data),'updatedAt',updated_at) FROM book_sources WHERE (? IS NULL OR book_id=?) ORDER BY is_primary DESC,id DESC",params![book,book])
}
fn resource_list(conn: &Connection, id: i64) -> rusqlite::Result<Vec<Value>> {
    rows(conn,"SELECT json_object('id',id,'bookId',book_id,'version',version,'state',state,'data',json(data),'updatedAt',updated_at) FROM digital_resources WHERE book_id=? ORDER BY id DESC",[id])
}
pub fn channel_list(conn: &Connection, id: i64) -> rusqlite::Result<Vec<Value>> {
    sync_legacy_channels(conn)?;
    rows(conn,"SELECT json_object('id',id,'bookId',book_id,'shop',shop,'productId',product_id,'data',json(data),'updatedAt',updated_at) FROM channel_products WHERE book_id=? AND app_key=? ORDER BY id DESC",params![id,app_key(conn)])
}
fn summary(conn: &Connection, b: &crate::models::Book) -> Result<Value, ApiError> {
    let profile = conn
        .query_row(
            "SELECT data FROM book_profiles WHERE book_id=?",
            [b.id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .map(parse)
        .unwrap_or(json!({}));
    let reqs=rows(conn,"SELECT json_object('school',school,'year',year,'major',major) FROM school_requirements WHERE book_id=? AND state='confirmed'",[b.id])?;
    let sources = source_list(conn, Some(b.id))?;
    let resources = resource_list(conn, b.id)?;
    let channels = channel_list(conn, b.id)?;
    let physical = sources
        .iter()
        .any(|v| text(&v["data"], "status") == "available" && v["data"]["dropship"] == true);
    let digital = resources.iter().any(|v| v["state"] == "ready");
    let mut v = json!(b);
    v["profile"] = profile;
    v["requirements"] = json!(reqs);
    v["physicalReady"] = json!(physical);
    v["digitalReady"] = json!(digital);
    v["channelCount"] = json!(channels.len());
    let deletion = crate::book_deletion::check(conn, b.id)?;
    v["canDelete"] = json!(deletion.can_delete);
    v["deleteReason"] = json!(deletion.reason);
    v["listed"] = json!(channels
        .iter()
        .any(|c| text(&c["data"], "state") == "published"));
    Ok(v)
}
async fn books(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let mut all = Vec::new();
    let mut stmt = c.prepare(&format!("{} ORDER BY b.id DESC", db::BOOK_SELECT))?;
    let ids = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        if let Some(b) = db::get_book(&c, id)? {
            all.push(summary(&c, &b)?);
        }
    }
    Ok(Json(json!(all)))
}
async fn book_detail(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    book_exists(&c, id)?;
    let mut v = summary(&c, &db::get_book(&c, id)?.unwrap())?;
    v["sources"] = json!(source_list(&c, Some(id))?);
    v["resources"] = json!(resource_list(&c, id)?);
    v["channels"] = json!(channel_list(&c, id)?);
    v["fulfillments"] = json!(fulfillment::all(&c)?
        .into_iter()
        .filter(|f| f["bookId"] == id)
        .collect::<Vec<_>>());
    v["schoolRecords"]=json!(rows(&c,"SELECT json_object('id',id,'state',state,'data',json(data)) FROM school_requirements WHERE book_id=?",[id])?);
    Ok(Json(v))
}
async fn save_profile(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    book_exists(&c, id)?;
    let year = text(&v, "publicationYear");
    if !year.is_empty() && !(year.len() == 4 && year.chars().all(|ch| ch.is_ascii_digit())) {
        return Err(ApiError::bad("出版年份应为四位数字"));
    }
    let raw:Option<String>=c.query_row("SELECT data FROM book_profiles WHERE book_id=?",[id],|r|r.get(0)).optional()?;
    let mut data=raw.map(parse).unwrap_or(json!({}));
    data["publicationYear"]=json!(year);data["volume"]=json!(text(&v,"volume"));data["confirmed"]=json!(v["confirmed"]==true);
    c.execute("INSERT INTO book_profiles(book_id,data) VALUES(?,?) ON CONFLICT(book_id) DO UPDATE SET data=excluded.data",params![id,data.to_string()])?;
    Ok(Json(data))
}
async fn sources(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(source_list(&lock(&s), None)?)))
}
async fn save_source(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let mut c = lock(&s);
    let tx = c.transaction()?;
    let bid = v["bookId"].as_i64();
    if let Some(b) = bid {
        book_exists(&tx, b)?;
    }
    let d = &v["data"];
    web_url(&required(d, "url")?)?;
    amount(d, "price", true)?;
    amount(d, "freight", true)?;
    if !["pending", "available", "unavailable"].contains(&text(d, "status")) {
        return Err(ApiError::bad("货源状态无效"));
    }
    if text(d, "status") == "available"
        && (bid.is_none()
            || text(d, "sku").is_empty()
            || text(d, "checkedAt").is_empty()
            || d["price"].is_null())
    {
        return Err(ApiError::bad(
            "可用货源须关联图书、填写规格和采购价，并完成核验",
        ));
    }
    let primary = v["primary"] == true;
    if primary && bid.is_none() {
        return Err(ApiError::bad("先关联图书再设主选货源"));
    }
    if primary {
        tx.execute(
            "UPDATE book_sources SET is_primary=0 WHERE book_id=?",
            [bid],
        )?;
    }
    let id = if let Some(id) = v["id"].as_i64() {
        let changed = tx.execute(
            "UPDATE book_sources SET book_id=?,is_primary=?,data=?,updated_at=? WHERE id=?",
            params![bid, primary, d.to_string(), db::now(), id],
        )?;
        if changed == 0 {
            return Err(ApiError::bad("货源不存在"));
        }
        id
    } else {
        tx.execute(
            "INSERT INTO book_sources(book_id,is_primary,data,updated_at) VALUES(?,?,?,?)",
            params![bid, primary, d.to_string(), db::now()],
        )?;
        tx.last_insert_rowid()
    };
    event(&tx, "source", id, "save", &json!({"bookId":bid}))?;
    if let Some(bid)=bid{crate::agents::ai::auto_queue(&tx,bid,"source")?;}
    tx.commit()?;
    Ok(Json(json!({"id":id})))
}
pub fn capture_candidate(
    conn: &Connection,
    capture: &crate::models::Capture,
) -> rusqlite::Result<()> {
    let data = json!({"url":capture.url,"title":capture.title,"shop":capture.shop,"price":capture.price,"freight":null,"sku":"","condition":"","status":"pending","dropship":false,"checkedAt":"","origin":"capture"});
    // Re-capture never overwrites a reviewed source's specification or negotiated price.
    conn.execute(
        "INSERT OR IGNORE INTO book_sources(capture_id,data,updated_at) VALUES(?,?,?)",
        params![capture.id, data.to_string(), db::now()],
    )?;
    Ok(())
}
pub fn attach_capture_candidate(conn: &Connection, capture: &crate::models::Capture, book_id: i64) -> rusqlite::Result<()> {
    // A listing may contain several editions. Never move a reviewed source to another book.
    let existing: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM book_sources WHERE book_id=? AND (capture_id=? OR json_extract(data,'$.originCaptureId')=?))",
        params![book_id,capture.id,capture.id],|r|r.get(0))?;
    if existing { return Ok(()); }
    if conn.execute("UPDATE book_sources SET book_id=? WHERE capture_id=? AND book_id IS NULL",params![book_id,capture.id])? == 0 {
        let data = json!({"url":capture.url,"title":capture.title,"shop":capture.shop,"price":capture.price,"freight":null,"sku":"","condition":"","status":"pending","dropship":false,"checkedAt":"","origin":"capture","originCaptureId":capture.id});
        conn.execute("INSERT INTO book_sources(book_id,data,updated_at) VALUES(?,?,?)",params![book_id,data.to_string(),db::now()])?;
    }
    crate::agents::ai::auto_queue(conn,book_id,"source")?;
    Ok(())
}
async fn add_resource(
    State(s): State<AppState>,
    Path(bid): Path<i64>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    book_exists(&c, bid)?;
    let version = required(&v, "version")?;
    web_url(&required(&v, "url")?)?;
    required(&v, "instructions")?;
    let data = json!({"url":text(&v,"url"),"code":text(&v,"code"),"instructions":text(&v,"instructions"),"edition":text(&v,"edition"),"hostingRef":text(&v,"hostingRef")});
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM digital_resources WHERE book_id=? AND version=?)",
        params![bid, version],
        |r| r.get(0),
    )?;
    if exists {
        return Err(ApiError::bad("资源版本已存在，修改内容请新增版本"));
    }
    c.execute("INSERT INTO digital_resources(book_id,version,data,created_at,updated_at) VALUES(?,?,?,?,?)",params![bid,version,data.to_string(),db::now(),db::now()])?;
    Ok(Json(json!({"id":c.last_insert_rowid()})))
}
async fn resource_state(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let status = required(&v, "state")?;
    if !["pending", "ready", "invalid", "retired"].contains(&status.as_str()) {
        return Err(ApiError::bad("资源状态无效"));
    }
    if status == "ready" && v["checked"] != true {
        return Err(ApiError::bad("请确认链接、内容及版次已核验"));
    }
    if c.execute(
        "UPDATE digital_resources SET state=?,updated_at=? WHERE id=?",
        params![status, db::now(), id],
    )? == 0
    {
        return Err(ApiError::bad("资源不存在"));
    }
    event(
        &c,
        "resource",
        id,
        &status,
        &json!({"note":text(&v,"note")}),
    )?;
    Ok(Json(json!({"ok":true})))
}

pub fn capability() -> Value {
    json!({"status":"unverified","canPublishMixed":false,"checks":[
    {"name":"同商品电子/实体双规格及订单规格识别","status":"needs_account_test"},
    {"name":"电子规格独立绑定可重复托管内容","status":"not_documented"},
    {"name":"实体规格排除自动发货且可正常物流发货","status":"needs_account_test"},
    {"name":"应用关闭期间自动交付","status":"needs_real_order_test"},
    {"name":"已有单规格商品原位升级","status":"needs_account_test"}],
    "message":"文档未提供完整的 SKU 级托管绑定契约，尚无真实订单证明关机自动发货。混合商品保持草稿，不自动拆分商品或由本地轮询代发。"})
}
pub fn check_channel(conn: &Connection, id: i64) -> Result<Value, ApiError> {
    let (bid, data): (i64, String) = conn
        .query_row(
            "SELECT book_id,data FROM channel_products WHERE id=? AND app_key=?",
            params![id, app_key(conn)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| ApiError::bad("渠道商品不存在"))?;
    let d = parse(data);
    let b = db::get_book(conn, bid)?.ok_or_else(|| ApiError::bad("图书不存在"))?;
    let mut errors = Vec::<String>::new();
    if b.isbn.trim().is_empty() {
        errors.push("缺少 ISBN".into())
    }
    if b.edition.trim().is_empty() {
        errors.push("版次尚未确认".into())
    }
    if text(&d, "title").is_empty() {
        errors.push("缺少商品标题".into())
    }
    if text(&d, "content").is_empty() {
        errors.push("缺少商品描述及各规格包含内容".into())
    }
    if db::book_image_rows(conn, bid)?.is_empty() {
        errors.push("缺少商品图片".into())
    }
    for (price, stock, label) in [
        ("physicalPrice", "physicalStock", "实体"),
        ("digitalPrice", "digitalStock", "电子"),
    ] {
        if label == "电子" && text(&d, "mode") != "mixed" {
            continue;
        }
        if d[price].as_f64().filter(|v| *v > 0.0).is_none() {
            errors.push(format!("缺少有效{label}售价"))
        }
        if d[stock].as_u64().is_none() {
            errors.push(format!("缺少{label}规格可售数量"))
        }
    }
    if !source_list(conn, Some(bid))?.iter().any(|v| {
        v["primary"] == true
            && text(&v["data"], "status") == "available"
            && v["data"]["dropship"] == true
    }) {
        errors.push("缺少已核验且支持代发的主选货源".into())
    }
    if text(&d, "mode") == "mixed" {
        let rid = d["resourceId"].as_i64().unwrap_or(0);
        if !resource_list(conn, bid)?
            .iter()
            .any(|v| v["id"] == rid && v["state"] == "ready")
        {
            errors.push("缺少已核验的电子资源版本".into())
        }
        errors.push(capability()["message"].as_str().unwrap().into());
    }
    Ok(json!({"ready":errors.is_empty(),"blockers":errors,"capability":capability()}))
}
async fn channel_check(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(check_channel(&lock(&s), id)?))
}
async fn save_channel(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let bid = v["bookId"]
        .as_i64()
        .ok_or_else(|| ApiError::bad("请选择图书"))?;
    book_exists(&c, bid)?;
    let shop = required(&v, "shop")?;
    let key = app_key(&c);
    if key.is_empty() {
        return Err(ApiError::bad("先配置闲管家应用"));
    }
    let d = &v["data"];
    if !["mixed", "physical"].contains(&text(d, "mode")) {
        return Err(ApiError::bad("商品形态无效"));
    }
    for name in ["physicalPrice", "digitalPrice"] {
        amount(d, name, true)?;
    }
    let existing:Option<(i64,String,String)>=c.query_row("SELECT id,product_id,data FROM channel_products WHERE app_key=? AND shop=? AND book_id=?",params![key,shop,bid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    for field in ["physicalStock", "digitalStock"] {
        if !d[field].is_null() && d[field].as_u64().filter(|n| *n <= 999999).is_none() {
            return Err(ApiError::bad("可售数量须为非负整数"));
        }
    }
    if v["createOnly"] == true && existing.is_some() {
        return Err(ApiError::bad("本书在该店铺已有草稿或商品，已跳过"));
    }
    let mut data = d.clone();
    data["state"] = json!("draft");
    // Remote identifiers can only be attached by the verified gateway endpoint.
    let product = existing
        .as_ref()
        .map(|(_, p, _)| p.clone())
        .unwrap_or_default();
    if let Some((_, _, old)) = &existing {
        let old = parse(old.clone());
        if !product.is_empty() && old["mode"] != data["mode"] {
            return Err(ApiError::bad(
                "已绑定远端商品的形态不能直接更改；原位升级须先验证平台能力",
            ));
        }
        for field in [
            "physicalSkuId",
            "digitalSkuId",
            "itemId",
            "verifiedMapping",
            "legacy",
        ] {
            data[field] = old[field].clone();
        }
        data["state"] = old["state"].clone();
    }
    if let Some(rid) = data["resourceId"].as_i64() {
        if !resource_list(&c, bid)?.iter().any(|r| r["id"] == rid) {
            return Err(ApiError::bad("电子资源不属于本书"));
        }
    }
    c.execute("INSERT INTO channel_products(book_id,app_key,shop,product_id,data,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(app_key,shop,book_id) DO UPDATE SET data=excluded.data,updated_at=excluded.updated_at",params![bid,key,shop,product,data.to_string(),db::now()])?;
    let id: i64 = c.query_row(
        "SELECT id FROM channel_products WHERE app_key=? AND shop=? AND book_id=?",
        params![key, shop, bid],
        |r| r.get(0),
    )?;
    Ok(Json(json!({"id":id,"check":check_channel(&c,id)?})))
}

async fn overview(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let mut tasks = Vec::new();
    let mut stmt = c.prepare("SELECT id FROM books ORDER BY id DESC")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        let b = db::get_book(&c, id)?.unwrap();
        let v = summary(&c, &b)?;
        let mut reasons = Vec::new();
        if v["profile"]["confirmed"] != true || b.isbn.is_empty() || b.edition.is_empty() {
            reasons.push("确认书目 / 补 ISBN 与版次")
        }
        if v["physicalReady"] != true {
            reasons.push("准备实体货源")
        }
        if v["digitalReady"] != true {
            reasons.push("准备电子资源")
        }
        if v["channelCount"] == 0 {
            reasons.push("准备渠道草稿")
        }
        for ch in channel_list(&c, id)? {
            let state = text(&ch["data"], "state");
            if [
                "draft",
                "created",
                "offline",
                "failed",
                "unknown",
                "preparing",
            ]
            .contains(&state)
            {
                let reason = if ["unknown", "preparing"].contains(&state) {
                    "发布结果待核查"
                } else if state == "draft" {
                    "完善销售草稿 / 校验发布阻塞项"
                } else {
                    "待核查或上架现有商品"
                };
                tasks.push(json!({"type":"book","bookId":id,"title":b.title,"reason":reason,"updatedAt":ch["updatedAt"]}));
            }
        }
        for resource in resource_list(&c, id)? {
            if resource["state"] == "invalid" {
                tasks.push(json!({"type":"book","bookId":id,"title":b.title,"reason":format!("电子资源失效：{}，核查历史订单并更新资源版本",text(&resource,"version")),"updatedAt":resource["updatedAt"]}));
            }
        }
        for reason in reasons {
            tasks.push(json!({"type":"book","bookId":id,"title":b.title,"reason":reason,"updatedAt":b.updated_at}));
        }
    }
    let pending: i64 = c.query_row(
        "SELECT count(*) FROM school_requirements WHERE state='pending' OR book_id IS NULL",
        [],
        |r| r.get(0),
    )?;
    let orders = fulfillment::all(&c)?;
    for f in &orders {
        if ![
            "closed",
            "completed",
            "delivered",
            "shipped",
            "waiting_payment",
        ]
        .contains(&text(f, "state"))
        {
            tasks.push(json!({"type":"order","orderNo":f["orderNo"],"fulfillmentId":f["id"],"bookId":f["bookId"],"title":f["snapshot"]["goods"]["title"],"reason":f["state"],"updatedAt":f["updatedAt"]}));
        }
    }
    let mut metrics = json!({"physical":{"revenue":0,"margin":0,"pendingCost":0,"cost":0,"fees":0,"refund":0},"digital":{"revenue":0,"margin":0,"pendingCost":0,"cost":0,"fees":0,"refund":0}});
    for f in &orders {
        let kind = text(f, "kind");
        if !["physical", "digital"].contains(&kind) {
            continue;
        }
        let status = f["snapshot"]["order_status"].as_i64().unwrap_or(0);
        if ![12, 21, 22, 23].contains(&status) {
            continue;
        }
        let income = f["snapshot"]["pay_amount"].as_i64().unwrap_or(0);
        let finance = &f["data"]["finance"];
        metrics[kind]["revenue"] = json!(metrics[kind]["revenue"].as_i64().unwrap() + income);
        let purchase = fulfillment::purchase_list(&c, f["id"].as_i64().unwrap())?;
        let cost = if kind == "physical" {
            let relevant: Vec<_> = purchase
                .iter()
                .filter(|p| p["state"] != "replaced")
                .collect();
            if relevant.is_empty() || relevant.iter().any(|p| p["state"] == "draft") {
                None
            } else {
                relevant.iter().try_fold(0.0, |sum, p| {
                    let paid = p["data"]["actualCost"].as_f64()?;
                    let refund = if p["state"] == "canceled" {
                        p["data"]["supplierRefund"].as_f64()?
                    } else {
                        0.0
                    };
                    Some(sum + paid - refund)
                })
            }
        } else {
            finance["cost"].as_f64()
        };
        for (name, value) in [
            ("cost", cost),
            ("fees", finance["fees"].as_f64()),
            ("refund", finance["refund"].as_f64()),
        ] {
            if let Some(value) = value {
                metrics[kind][name] = json!(
                    metrics[kind][name].as_i64().unwrap_or(0) + (value * 100.0).round() as i64
                );
            }
        }
        if let (Some(cost), Some(fee), Some(refund)) =
            (cost, finance["fees"].as_f64(), finance["refund"].as_f64())
        {
            metrics[kind]["margin"] = json!(
                metrics[kind]["margin"].as_i64().unwrap() + income
                    - ((cost + fee + refund) * 100.0).round() as i64
            )
        } else {
            metrics[kind]["pendingCost"] = json!(metrics[kind]["pendingCost"].as_i64().unwrap() + 1)
        }
    }
    Ok(Json(
        json!({"tasks":tasks,"pendingRequirements":pending,"metrics":metrics,"capability":capability()}),
    ))
}

pub fn sync_legacy_channels(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("INSERT OR IGNORE INTO channel_products(book_id,app_key,shop,product_id,data,updated_at) SELECT book_id,app_key,user_name,product_id,json_object('mode','physical','legacy',1,'physicalSkuId','0','digitalSkuId','','title',title,'itemId',item_id,'state',status),updated_at FROM xianyu_tasks WHERE product_id<>'' AND book_id IS NOT NULL ORDER BY id",[])?;
    // Adopt a published legacy task into a previously prepared single-spec draft, without duplicating it.
    conn.execute("UPDATE channel_products SET product_id=(SELECT t.product_id FROM xianyu_tasks t WHERE t.book_id=channel_products.book_id AND t.app_key=channel_products.app_key AND t.user_name=channel_products.shop AND t.product_id<>'' ORDER BY t.id LIMIT 1),data=json_set(data,'$.physicalSkuId','0','$.legacy',1),updated_at=? WHERE product_id='' AND json_extract(data,'$.mode')='physical' AND EXISTS(SELECT 1 FROM xianyu_tasks t WHERE t.book_id=channel_products.book_id AND t.app_key=channel_products.app_key AND t.user_name=channel_products.shop AND t.product_id<>'')",[db::now()])?;
    conn.execute("UPDATE channel_products SET data=json_set(data,'$.state',(SELECT t.status FROM xianyu_tasks t WHERE t.app_key=channel_products.app_key AND t.user_name=channel_products.shop AND t.product_id=channel_products.product_id ORDER BY t.id DESC LIMIT 1),'$.itemId',(SELECT t.item_id FROM xianyu_tasks t WHERE t.app_key=channel_products.app_key AND t.user_name=channel_products.shop AND t.product_id=channel_products.product_id ORDER BY t.id DESC LIMIT 1)) WHERE EXISTS(SELECT 1 FROM xianyu_tasks t WHERE t.app_key=channel_products.app_key AND t.user_name=channel_products.shop AND t.product_id=channel_products.product_id AND t.product_id<>'')",[])?;
    Ok(())
}
