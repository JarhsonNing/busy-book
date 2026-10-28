//! Local adapter: signing credentials stay on the backend; task diagnostics omit image payloads.
use crate::{
    api::ApiError,
    db, images, listing,
    models::ListingInput,
    state::{lock, AppState},
};
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path as FsPath, time::Duration};
use tokio::sync::Mutex;

pub struct Gateway {
    client: reqwest::Client,
    base: String,
    next_request: Mutex<tokio::time::Instant>,
    pub(crate) operation: Mutex<()>,
}

#[derive(Debug)]
struct RemoteError {
    message: String,
    rejected: bool,
}
impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigInput {
    app_key: String,
    #[serde(default)]
    app_secret: String,
}

impl Gateway {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(45))
                .build()?,
            base: "https://open.goofish.pro".into(),
            next_request: Mutex::new(tokio::time::Instant::now()),
            operation: Mutex::new(()),
        })
    }
    async fn call(
        &self,
        config: &ConfigInput,
        path: &str,
        request_body: &Value,
    ) -> Result<Value, RemoteError> {
        // Serialize exactly once: MD5 and HTTP must use the identical UTF-8 bytes.
        let raw = serde_json::to_string(request_body).expect("JSON value serializes");
        {
            let mut next = self.next_request.lock().await;
            tokio::time::sleep_until(*next).await;
            *next = tokio::time::Instant::now() + Duration::from_millis(125);
        }
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let signature = sign(&config.app_key, &config.app_secret, &timestamp, &raw);
        let response = self
            .client
            .post(format!("{}{}", self.base, path))
            .query(&[
                ("appid", config.app_key.as_str()),
                ("timestamp", timestamp.as_str()),
                ("sign", signature.as_str()),
            ])
            .header("content-type", "application/json")
            .body(raw)
            .send()
            .await
            .map_err(|_| RemoteError {
                message: "闲管家连接失败或超时；操作结果未知，请先核查远端状态".into(),
                rejected: false,
            })?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|_| RemoteError {
            message: format!("闲管家返回非 JSON 响应（HTTP {status}），操作结果未知"),
            rejected: false,
        })?;
        if !status.is_success() || body.get("code").and_then(Value::as_i64) != Some(0) {
            let msg = body
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("响应缺少成功状态码");
            let safe = diagnostic(msg, &config.app_secret, request_body);
            return Err(RemoteError {
                message: format!(
                    "闲管家 {}：{}",
                    body.get("code").unwrap_or(&Value::Null),
                    safe.chars().take(600).collect::<String>()
                ),
                rejected: status.is_success()
                    && body
                        .get("code")
                        .and_then(Value::as_i64)
                        .map(|code| code != 0 && !(500..600).contains(&code))
                        .unwrap_or(false),
            });
        }
        Ok(body.get("data").cloned().unwrap_or_else(|| body.clone()))
    }
}

fn diagnostic(message: &str, secret: &str, request: &Value) -> String {
    let mut safe = message.replace(secret, "[已隐藏]");
    fn omit(value: &Value, message: &mut String) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if key == "images" {
                        if let Some(images) = value.as_array() {
                            for image in images.iter().filter_map(Value::as_str) {
                                if !image.starts_with("http") {
                                    let prefix: String = image.chars().take(32).collect();
                                    if !prefix.is_empty() {
                                        if let Some(index) = message.find(&prefix) {
                                            message.truncate(index);
                                            message.push_str("[图片内容已省略]");
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        omit(value, message);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    omit(item, message);
                }
            }
            _ => {}
        }
    }
    omit(request, &mut safe);
    if let Some(index) = safe.find("data:image/") {
        safe.truncate(index);
        safe.push_str("[图片内容已省略]");
    }
    safe.chars().take(600).collect()
}

fn sign(app: &str, secret: &str, timestamp: &str, raw: &str) -> String {
    format!(
        "{:x}",
        md5::compute(format!(
            "{app},{:x},{timestamp},{secret}",
            md5::compute(raw)
        ))
    )
}

#[path = "commerce_gateway.rs"]
mod commerce_gateway;
#[path = "xianyu_orders.rs"]
mod orders;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(commerce_gateway::router())
        .route("/api/xianyu/config", get(config_get).put(config_put))
        .route("/api/xianyu/call/{operation}", post(dispatch))
        .route("/api/xianyu/tasks", get(tasks_get))
        .route(
            "/api/xianyu/orders/{order_no}/book",
            post(orders::link_book),
        )
        .route("/api/xianyu/books/{id}/orders", get(orders::book_orders))
        .route("/api/xianyu/books/{id}/preview", get(preview_book))
        .route("/api/xianyu/books/{id}/publish", post(publish_book))
        .route("/api/xianyu/tasks/{id}/refresh", post(refresh_task))
        .route("/api/xianyu/tasks/{id}/publish", post(resume_publish))
        .route("/api/xianyu/tasks/{id}/verify-images", post(verify_images))
}

fn config(state: &AppState) -> Result<ConfigInput, ApiError> {
    let conn = lock(state);
    let app_key = db::get_setting(&conn, "xianyuAppKey")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let app_secret = db::get_setting(&conn, "xianyuAppSecret")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    if app_key.is_empty() || app_secret.is_empty() {
        return Err(ApiError::bad("请先在闲鱼工作台配置 AppKey 和 AppSecret"));
    }
    Ok(ConfigInput {
        app_key,
        app_secret,
    })
}

async fn config_get(State(state): State<AppState>) -> Json<Value> {
    let conn = lock(&state);
    Json(
        json!({ "appKey": db::get_setting(&conn, "xianyuAppKey").unwrap_or(json!("")),
        "secretConfigured": db::get_setting(&conn, "xianyuAppSecret").and_then(|v| v.as_str().map(|s| !s.is_empty())).unwrap_or(false) }),
    )
}

async fn config_put(
    State(state): State<AppState>,
    Json(input): Json<ConfigInput>,
) -> Result<Json<Value>, ApiError> {
    let _guard = state
        .xianyu
        .operation
        .try_lock()
        .map_err(|_| ApiError::bad("当前有操作执行中，请稍后保存配置"))?;
    let key = input.app_key.trim();
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApiError::bad("AppKey 应为数字字符串"));
    }
    {
        let conn = lock(&state);
        let previous = db::get_setting(&conn, "xianyuAppKey").unwrap_or(Value::Null);
        if previous.as_str() != Some(key) && input.app_secret.trim().is_empty() {
            return Err(ApiError::bad("切换 AppKey 时请同时填写对应的 AppSecret"));
        }
        if !input.app_secret.trim().is_empty() {
            db::set_setting(&conn, "xianyuAppSecret", &json!(input.app_secret.trim()))?;
        }
        db::set_setting(&conn, "xianyuAppKey", &json!(key))?;
    }
    Ok(config_get(State(state.clone())).await)
}

// Keep all remote identifiers exact through JavaScript; convert only API integer IDs on input.
fn browser_ids(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if value.is_number()
                    && (key.ends_with("_id")
                        || key.ends_with("_no")
                        || value
                            .as_u64()
                            .map(|n| n > 9_007_199_254_740_991)
                            .unwrap_or(false))
                {
                    *value = Value::String(value.to_string());
                } else {
                    browser_ids(value);
                }
            }
        }
        Value::Array(items) => {
            for value in items {
                browser_ids(value);
            }
        }
        _ => {}
    }
}
fn integer_id(body: &mut Value, key: &str) -> Result<(), ApiError> {
    let id = body
        .get(key)
        .ok_or_else(|| ApiError::bad(format!("缺少 {key}")))?;
    let number = id
        .as_u64()
        .or_else(|| id.as_str().and_then(|s| s.parse::<u64>().ok()))
        .filter(|n| *n > 0)
        .ok_or_else(|| ApiError::bad(format!("{key} 必须为正整数 ID")))?;
    body[key] = json!(number);
    Ok(())
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ApiError::bad(format!("请填写 {key}")))
}
fn money(value: f64, allow_zero: bool) -> Result<i64, ApiError> {
    let cents = (value * 100.0).round();
    if !value.is_finite() || value < 0.0 || cents > 9_999_999_900.0 || (!allow_zero && cents < 1.0)
    {
        return Err(ApiError::bad("金额超出允许范围"));
    }
    Ok(cents as i64)
}

fn operation_body(op: &str, mut body: Value) -> Result<(&'static str, Value), ApiError> {
    if !body.is_object() {
        return Err(ApiError::bad("参数必须是 JSON 对象"));
    }
    let (path, fields): (&str, &[&str]) = match op {
        "shops" => ("/api/open/user/authorize/list", &[]),
        "addresses" => ("/api/open/user/address/list", &["type"]),
        "categories" => (
            "/api/open/product/category/list",
            &["item_biz_type", "sp_biz_type"],
        ),
        "properties" => (
            "/api/open/product/pv/list",
            &[
                "item_biz_type",
                "sp_biz_type",
                "channel_cat_id",
                "sub_property_id",
            ],
        ),
        "products" => (
            "/api/open/product/list",
            &["page_no", "page_size", "product_status", "update_time"],
        ),
        "product" => ("/api/open/product/detail", &["product_id"]),
        "edit" => (
            "/api/open/product/edit",
            &["product_id", "price", "publish_shop"],
        ),
        "stock" => ("/api/open/product/edit/stock", &["product_id", "stock"]),
        "publish" => ("/api/open/product/publish", &["product_id", "user_name"]),
        "unpublish" => ("/api/open/product/downShelf", &["product_id"]),
        "orders" => (
            "/api/open/order/list",
            &[
                "page_no",
                "page_size",
                "order_status",
                "authorize_id",
                "update_time",
            ],
        ),
        "order" => ("/api/open/order/detail", &["order_no"]),
        "ship" => (
            "/api/open/order/ship",
            &[
                "order_no",
                "waybill_no",
                "express_code",
                "express_name",
                "ship_name",
                "ship_mobile",
                "ship_district_id",
                "ship_address",
            ],
        ),
        "companies" => ("/api/open/express/companies", &[]),
        _ => return Err(ApiError::bad("不支持的闲鱼操作")),
    };
    body.as_object_mut()
        .unwrap()
        .retain(|key, _| fields.contains(&key.as_str()));
    if fields.contains(&"product_id") {
        integer_id(&mut body, "product_id")?;
    }
    if body.get("authorize_id").is_some() {
        integer_id(&mut body, "authorize_id")?;
    }
    for key in ["page_no", "page_size"] {
        if let Some(v) = body.get(key) {
            if !v.as_u64().map(|n| (1..=100).contains(&n)).unwrap_or(false) {
                return Err(ApiError::bad("分页范围为 1–100"));
            }
        }
    }
    if op == "stock"
        && !body
            .get("stock")
            .and_then(Value::as_u64)
            .map(|n| n <= 9999)
            .unwrap_or(false)
    {
        return Err(ApiError::bad("库存范围为 0–9999"));
    }
    if op == "publish"
        && !body
            .get("user_name")
            .and_then(Value::as_array)
            .map(|a| a.len() == 1 && a[0].as_str().map(|s| !s.is_empty()).unwrap_or(false))
            .unwrap_or(false)
    {
        return Err(ApiError::bad("请选择一个上架店铺"));
    }
    if ["order", "ship"].contains(&op) {
        text(&body, "order_no")?;
    }
    if op == "edit" {
        if let Some(v) = body.get("price") {
            if !v
                .as_u64()
                .map(|n| (1..=9_999_999_900).contains(&n))
                .unwrap_or(false)
            {
                return Err(ApiError::bad("价格必须以整数分传入"));
            }
        }
        if let Some(shops) = body.get("publish_shop") {
            let shops = shops
                .as_array()
                .filter(|a| a.len() == 1)
                .ok_or_else(|| ApiError::bad("编辑时请选择一个店铺"))?;
            for shop in shops {
                text(shop, "user_name")?;
                validate_copy(text(shop, "title")?, text(shop, "content")?)?;
            }
        }
    }
    if op == "ship" {
        for key in ["waybill_no", "express_code", "express_name"] {
            text(&body, key)?;
        }
        let sender = [
            "ship_name",
            "ship_mobile",
            "ship_district_id",
            "ship_address",
        ];
        if sender.iter().any(|key| body.get(*key).is_some()) {
            for key in ["ship_name", "ship_mobile", "ship_address"] {
                text(&body, key)?;
            }
            integer_id(&mut body, "ship_district_id")?;
        }
    }
    Ok((path, body))
}

async fn dispatch(
    State(state): State<AppState>,
    Path(op): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let (path, body) = operation_body(&op, body)?;
    let cfg = config(&state)?;
    let _guard = if ["edit", "stock", "publish", "unpublish", "ship"].contains(&op.as_str()) {
        Some(
            state
                .xianyu
                .operation
                .try_lock()
                .map_err(|_| ApiError::bad("已有操作执行中，请稍后重试"))?,
        )
    } else {
        None
    };
    let mut data = state
        .xianyu
        .call(&cfg, path, &body)
        .await
        .map_err(|e| ApiError::bad(e.message))?;
    if op == "orders" {
        if let Some(list) = data.get_mut("list").and_then(Value::as_array_mut) {
            for order in list {
                orders::enrich(&state, &cfg.app_key, order)?;
            }
        }
    } else if op == "order" {
        orders::enrich(&state, &cfg.app_key, &mut data)?;
        orders::inventory(&state, &cfg, &mut data).await;
    }
    if op == "products" {
        if let Some(list) = data.get_mut("list").and_then(Value::as_array_mut) {
            for product in list {
                orders::enrich_product(&state, &cfg.app_key, product)?;
            }
        }
    } else if op == "product" {
        orders::enrich_product(&state, &cfg.app_key, &mut data)?;
    }
    browser_ids(&mut data);
    Ok(Json(data))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Task {
    id: i64,
    book_id: Option<i64>,
    user_name: String,
    title: String,
    product_id: String,
    item_id: String,
    status: String,
    image_mode: String,
    images_verified: bool,
    error: String,
    attempts: Value,
    updated_at: String,
}
fn task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let attempts: String = row.get("attempts")?;
    Ok(Task {
        id: row.get("id")?,
        book_id: row.get("book_id")?,
        user_name: row.get("user_name")?,
        title: row.get("title")?,
        product_id: row.get("product_id")?,
        item_id: row.get("item_id")?,
        status: row.get("status")?,
        image_mode: row.get("image_mode")?,
        images_verified: row.get::<_, i64>("images_verified")? != 0,
        error: row.get("error")?,
        attempts: serde_json::from_str(&attempts).unwrap_or(json!([])),
        updated_at: row.get("updated_at")?,
    })
}
fn task(state: &AppState, id: i64, key: &str) -> Result<Task, ApiError> {
    lock(state)
        .query_row(
            "SELECT * FROM xianyu_tasks WHERE id=? AND app_key=?",
            params![id, key],
            task_row,
        )
        .optional()?
        .ok_or_else(|| ApiError::bad("发布记录不存在或属于其他应用"))
}
fn task_state(state: &AppState, id: i64, status: &str, error: &str) -> Result<(), ApiError> {
    lock(state).execute(
        "UPDATE xianyu_tasks SET status=?,error=?,updated_at=? WHERE id=?",
        params![status, error, db::now(), id],
    )?;
    Ok(())
}
fn attempt(state: &AppState, id: i64, mode: &str, result: &str) -> Result<(), ApiError> {
    let conn = lock(state);
    let raw: String =
        conn.query_row("SELECT attempts FROM xianyu_tasks WHERE id=?", [id], |r| {
            r.get(0)
        })?;
    let mut entries: Vec<Value> = serde_json::from_str(&raw).unwrap_or_default();
    entries.push(json!({"mode":mode,"result":result,"time":db::now()}));
    conn.execute(
        "UPDATE xianyu_tasks SET image_mode=?,attempts=?,updated_at=? WHERE id=?",
        params![mode, json!(entries).to_string(), db::now(), id],
    )?;
    Ok(())
}
async fn tasks_get(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let key = config(&state)?.app_key;
    let conn = lock(&state);
    let mut stmt =
        conn.prepare("SELECT * FROM xianyu_tasks WHERE app_key=? ORDER BY id DESC LIMIT 100")?;
    let list = stmt
        .query_map([key], task_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Json(json!(list)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishInput {
    #[serde(default)]
    sale_mode: String,
    user_name: String,
    channel_cat_id: String,
    province: i64,
    city: i64,
    district: i64,
    price: f64,
    express_fee: f64,
    stock: i64,
    stuff_status: i64,
    title: String,
    description: String,
    #[serde(default)]
    isbn: Option<String>,
    #[serde(default)]
    channel_pv: Vec<Value>,
    #[serde(default = "default_mode")]
    image_mode: String,
    #[serde(default)]
    image_urls: Vec<String>,
    #[serde(default)]
    create_only: bool,
    #[serde(default)]
    first_image_only: bool,
}
fn default_mode() -> String {
    "data-url".into()
}
fn normalize_isbn(value: &str) -> Result<String, ApiError> {
    let isbn: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if isbn.is_empty() {
        return Err(ApiError::bad("图书 ISBN 为必填项，请在发布窗口补填 ISBN"));
    }
    let bytes = isbn.as_bytes();
    let valid = match bytes.len() {
        10 => {
            bytes[..9].iter().all(u8::is_ascii_digit)
                && (bytes[9].is_ascii_digit() || bytes[9] == b'X')
        }
        13 => {
            (isbn.starts_with("978") || isbn.starts_with("979"))
                && bytes.iter().all(u8::is_ascii_digit)
        }
        _ => false,
    };
    if !valid {
        return Err(ApiError::bad(
            "ISBN 格式错误，请填写 10 位 ISBN 或以 978/979 开头的 13 位 ISBN",
        ));
    }
    Ok(isbn)
}
fn validate_copy(title: &str, content: &str) -> Result<(), ApiError> {
    let length = |s: &str| {
        s.chars()
            .map(|c| if c.is_ascii() { 1 } else { 2 })
            .sum::<usize>()
    };
    if title.trim().is_empty() || length(title) > 60 {
        return Err(ApiError::bad("标题最多 60 字符（中文计 2 个）"));
    }
    if !(5..=5000).contains(&length(content)) {
        return Err(ApiError::bad("描述须为 5–5000 字符（中文计 2 个）"));
    }
    Ok(())
}
fn image_error(error: &RemoteError) -> bool {
    let msg = error.message.to_lowercase();
    error.rejected
        && ["图片", "images", "image url", "base64"]
            .iter()
            .any(|term| msg.contains(term))
}
fn base64_image(bytes: &[u8], mime: &str, raw: bool) -> String {
    let encoded = STANDARD.encode(bytes);
    if raw {
        encoded
    } else {
        format!("data:{mime};base64,{encoded}")
    }
}

async fn publish_book(
    State(state): State<AppState>,
    Path(book_id): Path<i64>,
    Json(input): Json<PublishInput>,
) -> Result<Json<Value>, ApiError> {
    let _guard = state
        .xianyu
        .operation
        .try_lock()
        .map_err(|_| ApiError::bad("已有操作执行中，请稍后重试"))?;
    let cfg = config(&state)?;
    if input.sale_mode == "mixed" {
        return Err(ApiError::bad(
            crate::commerce::capability()["message"].as_str().unwrap(),
        ));
    }
    if !input.sale_mode.is_empty() && input.sale_mode != "physical" {
        return Err(ApiError::bad("售卖形态无效"));
    }
    validate_copy(&input.title, &input.description)?;
    if input.user_name.trim().is_empty()
        || input.channel_cat_id.trim().is_empty()
        || input.province <= 0
        || input.city <= 0
        || input.district <= 0
    {
        return Err(ApiError::bad("请选择店铺、图书类目和完整发货地区"));
    }
    if !crate::regions::valid(input.province, input.city, input.district) {
        return Err(ApiError::bad("发货省市区编码不匹配，请重新选择省、市、区"));
    }
    if !(1..=9999).contains(&input.stock) {
        return Err(ApiError::bad("库存范围为 1–9999"));
    }
    if ![0, 100, -1, 99, 95, 90, 80, 70, 60, 50].contains(&input.stuff_status) {
        return Err(ApiError::bad("成色无效"));
    }
    let price = money(input.price, false)?;
    let express_fee = money(input.express_fee, true)?;
    let (book, rows) = {
        let conn = lock(&state);
        let channel: Option<(String,String)> = conn.query_row("SELECT product_id,data FROM channel_products WHERE book_id=? AND app_key=? AND shop=?",params![book_id,cfg.app_key,input.user_name],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((product, data)) = channel {
            if !product.is_empty() {
                return Err(ApiError::bad(
                    "本书在该店铺已有渠道商品，请查看现有商品，不能重复创建",
                ));
            }
            if crate::commerce::parse(data)["mode"] == "mixed" {
                return Err(ApiError::bad(
                    crate::commerce::capability()["message"].as_str().unwrap(),
                ));
            }
        }
        let existing: Option<i64> = conn.query_row("SELECT id FROM xianyu_tasks WHERE book_id=? AND app_key=? AND user_name=? AND (product_id<>'' OR status IN ('preparing','unknown')) ORDER BY id DESC LIMIT 1", params![book_id,cfg.app_key,input.user_name], |r| r.get(0)).optional()?;
        if let Some(id) = existing {
            return Err(ApiError::bad(format!(
                "已有接口发布记录 #{id}，请到发布记录核查或继续上架，避免重复创建"
            )));
        }
        (
            db::get_book(&conn, book_id)?.ok_or_else(|| ApiError::bad("图书不存在"))?,
            db::book_image_rows(&conn, book_id)?,
        )
    };
    let isbn = normalize_isbn(input.isbn.as_deref().unwrap_or(&book.isbn))?;
    // Confirm the selected shop is still authorized before creating any remote product.
    let shops = state
        .xianyu
        .call(&cfg, "/api/open/user/authorize/list", &json!({}))
        .await
        .map_err(|e| ApiError::bad(e.message))?;
    let shop = shops["list"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|s| s["user_name"].as_str() == Some(&input.user_name))
        })
        .ok_or_else(|| ApiError::bad("店铺不属于当前应用"))?;
    if shop["is_valid"] == false
        || shop["authorize_expires"]
            .as_i64()
            .map(|n| n < chrono::Utc::now().timestamp())
            .unwrap_or(false)
    {
        return Err(ApiError::bad("店铺权益或授权已过期，请在闲管家处理后重试"));
    }
    let mut image_values = Vec::new();
    match input.image_mode.as_str() {
        "data-url" | "raw-base64" => {
            let mut sorted = rows;
            sorted.sort_by_key(|r| (-r.is_primary, r.sort, r.id));
            let limit = if input.first_image_only { 1 } else { 9 };
            let mut total = 0;
            for row in sorted.iter().take(limit) {
                let name = if row.processed_file.is_empty() {
                    &row.origin_file
                } else {
                    &row.processed_file
                };
                let name = FsPath::new(name)
                    .file_name()
                    .ok_or_else(|| ApiError::bad("图片文件名无效"))?;
                let bytes = std::fs::read(state.image_dir.join(name))?;
                total += bytes.len();
                if total > 20 * 1024 * 1024 {
                    return Err(ApiError::bad(
                        "本次图片合计超过 20MB，请压缩图片或只测试首图",
                    ));
                }
                image_values.push(base64_image(
                    &bytes,
                    images::mime_for(&name.to_string_lossy()),
                    input.image_mode == "raw-base64",
                ));
            }
        }
        "gitee" => {
            image_values =
                crate::gitee::upload_book(&state, book_id, &rows, input.first_image_only).await?;
        }
        "url" => {
            for url in input.image_urls.iter().take(9) {
                let parsed =
                    reqwest::Url::parse(url).map_err(|_| ApiError::bad("图片链接格式错误"))?;
                let host = parsed.host_str().unwrap_or("");
                if !["http", "https"].contains(&parsed.scheme())
                    || host.is_empty()
                    || host == "localhost"
                    || host == "::1"
                    || host.starts_with("127.")
                    || host.starts_with("192.168.")
                    || host.starts_with("10.")
                {
                    return Err(ApiError::bad("图片须使用公网 HTTP/HTTPS 链接"));
                }
                image_values.push(url.clone());
            }
        }
        _ => return Err(ApiError::bad("未知图片模式")),
    }
    if image_values.is_empty() {
        return Err(ApiError::bad("至少需要一张图片"));
    }
    let id = {
        let conn = lock(&state);
        conn.execute("INSERT INTO xianyu_tasks(book_id,app_key,user_name,title,image_mode,created_at,updated_at) VALUES(?,?,?,?,?,?,?)", params![book_id,cfg.app_key,input.user_name,input.title,input.image_mode,db::now(),db::now()])?;
        conn.last_insert_rowid()
    };
    let mut payload = json!({"item_biz_type":2,"sp_biz_type":24,"channel_cat_id":input.channel_cat_id,
        "price":price,"express_fee":express_fee,"stock":input.stock,"stuff_status":input.stuff_status,
        "outer_id":format!("taoshu-{book_id}-{id}"),"channel_pv":input.channel_pv,
        "publish_shop":[{"user_name":input.user_name,"province":input.province,"city":input.city,"district":input.district,
            "title":input.title,"content":input.description,"images":image_values}],
        "book_data":{"title":book.title,"author":book.author,"publisher":book.publisher,"isbn":isbn}});
    let mut mode = input.image_mode.clone();
    let mut result = state
        .xianyu
        .call(&cfg, "/api/open/product/create", &payload)
        .await;
    if let Err(error) = &result {
        // Retry only an explicit image rejection, never a timeout or ambiguous HTTP failure.
        if mode == "data-url" && image_error(error) {
            attempt(&state, id, &mode, &error.message)?;
            mode = "raw-base64".into();
            let images = payload["publish_shop"][0]["images"].as_array_mut().unwrap();
            for value in images {
                if let Some((_, raw)) = value.as_str().and_then(|s| s.split_once(',')) {
                    *value = json!(raw);
                }
            }
            result = state
                .xianyu
                .call(&cfg, "/api/open/product/create", &payload)
                .await;
        }
    }
    match result {
        Ok(data) => {
            let product_id = data["product_id"]
                .as_u64()
                .map(|n| n.to_string())
                .or_else(|| data["product_id"].as_str().map(str::to_string))
                .unwrap_or_default();
            if product_id.is_empty() {
                task_state(
                    &state,
                    id,
                    "unknown",
                    "创建响应缺少 product_id，请核查远端记录",
                )?;
            } else {
                lock(&state).execute(
                    "UPDATE xianyu_tasks SET product_id=?,status='created',error='' WHERE id=?",
                    params![product_id, id],
                )?;
                attempt(&state, id, &mode, "创建成功；图片显示效果待核验")?;
                if !input.create_only {
                    submit_publish(&state, &cfg, id).await?;
                }
            }
        }
        Err(error) => {
            let status = if !error.rejected {
                "unknown"
            } else if image_error(&error) {
                "base64_rejected"
            } else {
                "failed"
            };
            attempt(&state, id, &mode, &error.message)?;
            task_state(&state, id, status, &error.message)?;
        }
    }
    Ok(Json(json!(task(&state, id, &cfg.app_key)?)))
}

async fn submit_publish(state: &AppState, cfg: &ConfigInput, id: i64) -> Result<(), ApiError> {
    let t = task(state, id, &cfg.app_key)?;
    let mut payload = json!({"product_id":t.product_id,"user_name":[t.user_name]});
    integer_id(&mut payload, "product_id")?;
    // Persist before sending; a crash or network error cannot cause a blind resend.
    task_state(state, id, "publishing", "")?;
    match state
        .xianyu
        .call(cfg, "/api/open/product/publish", &payload)
        .await
    {
        Ok(_) => {
            attempt(state, id, &t.image_mode, "已受理上架；等待远端确认")?;
        }
        Err(e) => {
            task_state(
                state,
                id,
                if e.rejected { "failed" } else { "unknown" },
                &e.message,
            )?;
            attempt(state, id, &t.image_mode, &e.message)?;
        }
    }
    Ok(())
}

async fn refresh_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let _guard = state
        .xianyu
        .operation
        .try_lock()
        .map_err(|_| ApiError::bad("已有操作执行中，请稍后刷新"))?;
    let cfg = config(&state)?;
    refresh(&state, &cfg, id).await?;
    Ok(Json(json!(task(&state, id, &cfg.app_key)?)))
}
async fn refresh(state: &AppState, cfg: &ConfigInput, id: i64) -> Result<(), ApiError> {
    let mut t = task(state, id, &cfg.app_key)?;
    if t.product_id.is_empty() && ["unknown", "preparing"].contains(&t.status.as_str()) {
        let code = format!(
            "taoshu-{}-{id}",
            t.book_id
                .ok_or_else(|| ApiError::bad("本地图书已删除，请在闲管家核查商品"))?
        );
        let since: String = lock(state).query_row(
            "SELECT created_at FROM xianyu_tasks WHERE id=?",
            [id],
            |r| r.get(0),
        )?;
        let from = chrono::DateTime::parse_from_rfc3339(&since)
            .map(|d| d.timestamp() - 60)
            .unwrap_or(0);
        for page in 1..=100 {
            let data = state.xianyu.call(cfg,"/api/open/product/list",&json!({"page_no":page,"page_size":100,"update_time":[from,chrono::Utc::now().timestamp()]})).await.map_err(|e|ApiError::bad(e.message))?;
            let list = data["list"]
                .as_array()
                .ok_or_else(|| ApiError::bad("商品列表响应缺少 list"))?;
            if let Some(found) = list.iter().find(|v| v["outer_id"].as_str() == Some(&code)) {
                let product_id = found["product_id"]
                    .as_u64()
                    .map(|n| n.to_string())
                    .or_else(|| found["product_id"].as_str().map(str::to_string))
                    .unwrap_or_default();
                if !product_id.is_empty() {
                    lock(state).execute(
                        "UPDATE xianyu_tasks SET product_id=?,status='created',error='' WHERE id=?",
                        params![product_id, id],
                    )?;
                    t = task(state, id, &cfg.app_key)?;
                }
                break;
            }
            if list.len() < 100 {
                break;
            }
        }
        if t.product_id.is_empty() {
            task_state(
                state,
                id,
                "unknown",
                "暂未查到对应商品，请稍后再次核查或到闲管家按商家编码核查；不要重复创建",
            )?;
            return Ok(());
        }
    }
    if t.product_id.is_empty() {
        return Ok(());
    }
    let mut body = json!({"product_id":t.product_id});
    integer_id(&mut body, "product_id")?;
    let data = state
        .xianyu
        .call(cfg, "/api/open/product/detail", &body)
        .await
        .map_err(|e| ApiError::bad(e.message))?;
    let (status, item_id, error) = remote_state(&data, &t.user_name);
    if let Some(status) = status {
        // A freshly accepted publish may still report '待发布'; retain pending until confirmed or timed out.
        let pending = t.status == "publishing" && status == "created";
        let expired = chrono::DateTime::parse_from_rfc3339(&t.updated_at)
            .map(|d| chrono::Utc::now().timestamp() - d.timestamp() > 300)
            .unwrap_or(true);
        if !pending || expired {
            task_state(
                state,
                id,
                if pending { "unknown" } else { status },
                if pending {
                    "上架已受理但尚未确认，请核查远端状态"
                } else {
                    error
                },
            )?;
        }
        if !item_id.is_empty() {
            lock(state).execute(
                "UPDATE xianyu_tasks SET item_id=? WHERE id=?",
                params![item_id, id],
            )?;
        }
        if status == "published" {
            if let Some(book_id) = t.book_id {
                lock(state).execute(
                    "UPDATE books SET status='listed',updated_at=? WHERE id=?",
                    params![db::now(), book_id],
                )?;
            }
        }
    }
    Ok(())
}
fn remote_state<'a>(data: &'a Value, user: &str) -> (Option<&'static str>, String, &'a str) {
    let shop = data["publish_shop"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["user_name"].as_str() == Some(user)));
    let item = shop
        .and_then(|s| s.get("item_id"))
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string())
        })
        .filter(|s| s != "0" && s != "null")
        .unwrap_or_default();
    if data["product_status"] == -1 {
        return (Some("deleted"), item, "");
    }
    let active_shop = shop.map(|s| s["status"] == 1).unwrap_or(false);
    let state = match data["publish_status"].as_i64() {
        Some(3) if active_shop && !item.is_empty() => Some("published"),
        Some(9) => Some("failed"),
        Some(4 | 5) => Some("offline"),
        Some(1 | 2) => Some("created"),
        _ if data["product_status"] == 22 && active_shop && !item.is_empty() => Some("published"),
        _ if data["product_status"] == -1 => Some("deleted"),
        _ => None,
    };
    (
        state,
        item,
        if state == Some("failed") {
            "远端商品异常，请在商品详情或闲管家查看处理结果"
        } else {
            ""
        },
    )
}
async fn resume_publish(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let _guard = state
        .xianyu
        .operation
        .try_lock()
        .map_err(|_| ApiError::bad("已有操作执行中"))?;
    let cfg = config(&state)?;
    let before = task(&state, id, &cfg.app_key)?;
    if before.status == "publishing" {
        return Err(ApiError::bad("上架处理中，请先刷新核查"));
    }
    refresh(&state, &cfg, id).await?;
    let t = task(&state, id, &cfg.app_key)?;
    if !["created", "offline", "failed"].contains(&t.status.as_str()) || t.product_id.is_empty() {
        return Err(ApiError::bad("当前状态不可重复上架，请核查远端结果"));
    }
    submit_publish(&state, &cfg, id).await?;
    Ok(Json(json!(task(&state, id, &cfg.app_key)?)))
}
async fn verify_images(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let cfg = config(&state)?;
    let t = task(&state, id, &cfg.app_key)?;
    if t.status != "published" {
        return Err(ApiError::bad("上架成功后再确认图片"));
    }
    lock(&state).execute(
        "UPDATE xianyu_tasks SET images_verified=1,updated_at=? WHERE id=?",
        params![db::now(), id],
    )?;
    Ok(Json(json!(task(&state, id, &cfg.app_key)?)))
}

async fn preview_book(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(preview(&state, id)?))
}

// Preview shares the existing book copy builder; no remote writes.
pub fn preview(state: &AppState, book_id: i64) -> Result<Value, ApiError> {
    let conn = lock(state);
    let book = db::get_book(&conn, book_id)?.ok_or_else(|| ApiError::bad("图书不存在"))?;
    let settings = db::settings(&conn);
    let images = db::images_for_book(&conn, book_id)?;
    let input = ListingInput {
        title: None,
        description: None,
        price: None,
        condition: None,
    };
    let (built, _) = listing::build(&book, &settings, &images, &input, &state.base_url());
    Ok(
        json!({"title":built.title,"description":built.description,"price":built.price,"stock":book.stock,"condition":book.condition}),
    )
}

#[cfg(test)]
#[path = "xianyu_tests.rs"]
mod tests;
