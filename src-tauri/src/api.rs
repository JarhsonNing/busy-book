use std::{collections::HashMap, path::Path as FsPath};

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};

use crate::{
    db, images, listing,
    models::*,
    state::{lock, AppState},
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/* --------------------------------- 错误类型 -------------------------------- */

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    pub(crate) message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    pub(crate) fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
    fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }
    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(value: rusqlite::Error) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("数据库错误: {value}"),
        )
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{value:#}"))
    }
}

impl From<std::io::Error> for ApiError {
    fn from(value: std::io::Error) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("文件错误: {value}"),
        )
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(value: serde_json::Error) -> Self {
        Self::bad(format!("参数错误: {value}"))
    }
}

/* ---------------------------------- 启动 ---------------------------------- */

pub async fn serve(state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", state.port)).await?;
    println!("[淘书工坊] 本地接口已启动: {}", state.base_url());
    crate::commerce::jobs::start(state.clone());
    axum::serve(listener, router(state)).await?;
    Ok(())
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/token", get(token))
        .route("/api/stats", get(stats))
        .route("/api/settings", get(settings_get).put(settings_put))
        .route("/api/captures", post(create_capture))
        .route("/api/books", get(books_list).post(books_create))
        .route("/api/books/delete-preview", post(books_delete_preview))
        .route("/api/books/delete", post(books_delete))
        .route(
            "/api/books/{id}",
            get(book_get).patch(book_patch).delete(book_delete),
        )
        .route("/api/books/{id}/images", post(images_import))
        .route("/api/books/{id}/upload", post(image_upload))
        .route("/api/books/{id}/images/order", put(images_order))
        .route("/api/images/{id}/file", get(image_file))
        .route("/api/images/{id}/processed", post(image_processed))
        .route("/api/images/{id}/primary", post(image_primary))
        .route("/api/images/{id}", delete(image_delete))
        .route(
            "/api/import-context",
            get(import_get).post(import_set).delete(import_clear),
        )
        .route("/api/books/{id}/listing", post(listing_create))
        .route("/api/listings/{id}/payload", get(listing_payload))
        .route("/api/listings/{id}/report", post(listing_report))
        .route("/api/export.csv", get(export_csv))
        .merge(crate::xianyu::router())
        .merge(crate::gitee::router())
        .merge(crate::commerce::router())
        .merge(crate::agents::router())
        .fallback(not_found)
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(40 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}

/* ---------------------------------- 鉴权 ---------------------------------- */

fn header_eq(headers: &HeaderMap, name: &str, expected: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value == expected)
        .unwrap_or(false)
}

async fn authorize(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let path = req.uri().path().to_string();
    if path == "/mcp" {
        if !crate::agents::mcp::origin_allowed(req.headers()) { return Err(ApiError::forbidden("MCP 来源不受信任")); }
        if !crate::agents::mcp::authorized(&state,req.headers()) { return Err(ApiError::unauthorized("MCP 未开启或导入令牌无效")); }
        return Ok(next.run(req).await);
    }
    let is_public = req.method() == Method::OPTIONS
        || path == "/api/health"
        || path == "/api/token"
        || (path.starts_with("/api/images/") && path.ends_with("/file"));

    if is_public {
        return Ok(next.run(req).await);
    }

    let with_token = header_eq(req.headers(), "x-kf-token", &state.token);
    let with_secret = header_eq(req.headers(), "x-taoshu-secret", &state.bridge_secret);
    if with_token || with_secret {
        Ok(next.run(req).await)
    } else {
        Err(ApiError::unauthorized("缺少访问令牌"))
    }
}

fn origin_trusted(origin: &str) -> bool {
    if origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://") {
        return true;
    }
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or(origin);
    let host = rest.split('/').next().unwrap_or("");
    host == "tauri.localhost"
        || host
            .strip_prefix("tauri.localhost:")
            .map(|port| port.parse::<u16>().is_ok())
            .unwrap_or(false)
        || host == "localhost:1420"
        || host == "127.0.0.1:1420"
        || host == "localhost"
        || host == "127.0.0.1"
}

/* --------------------------------- 基础接口 -------------------------------- */

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "version": VERSION,
        "baseUrl": state.base_url(),
    }))
}

async fn token(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !origin.is_empty() && !origin_trusted(origin) {
        return Err(ApiError::forbidden("来源不受信任"));
    }
    Ok(Json(json!({
        "token": state.token,
        "baseUrl": state.base_url(),
        "version": VERSION,
    })))
}

async fn not_found() -> ApiError {
    ApiError::not_found("接口不存在")
}

async fn stats(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    Ok(Json(db::stats(&conn)?))
}

async fn settings_get(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    Ok(Json(serde_json::to_value(db::settings(&conn))?))
}

async fn settings_put(
    State(state): State<AppState>,
    Json(patch): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    Ok(Json(serde_json::to_value(db::save_settings(
        &conn, &patch,
    )?)?))
}

/* ---------------------------------- 采集 ---------------------------------- */

async fn create_capture(
    State(state): State<AppState>,
    Json(input): Json<CaptureInput>,
) -> Result<Json<Value>, ApiError> {
    if let Some(id) = input.source_book_id {
        if !input.as_source || db::get_book(&lock(&state), id)?.is_none() {
            return Err(ApiError::bad("货源目标图书无效"));
        }
    }
    let settings = {
        let conn = lock(&state);
        db::settings(&conn)
    };
    let (capture, book, created) = {
        let conn = lock(&state);
        db::upsert_capture(
            &conn,
            &input,
            settings.profit_rate,
            settings.min_profit,
            &settings.default_condition,
        )?
    };

    crate::commerce::capture_candidate(&lock(&state), &capture)?;
    if let Some(id) = input.source_book_id {
        crate::commerce::attach_capture_candidate(&lock(&state), &capture, id)?;
    }
    let book_id = book.as_ref().map(|value| value.id);
    if let Some(book_id) = book_id {
        let main = input
            .main_image
            .clone()
            .or_else(|| input.images.as_ref().and_then(|list| list.first().cloned()))
            .unwrap_or_default();
        if !main.is_empty() {
            let exists = {
                let conn = lock(&state);
                conn.query_row(
                    "SELECT 1 FROM images WHERE book_id = ? AND origin_url = ?",
                    params![book_id, main],
                    |_| Ok(()),
                )
                .optional()?
                .is_some()
            };
            if !exists {
                if let Ok(saved) =
                    images::download(&main, "https://item.taobao.com/", &state.image_dir).await
                {
                    let conn = lock(&state);
                    let _ = db::add_image(
                        &conn,
                        book_id,
                        "taobao",
                        &main,
                        &saved.file,
                        saved.width,
                        saved.height,
                    );
                }
            }
        }
    }

    let detail = {
        let conn = lock(&state);
        match book_id {
            Some(id) => {crate::agents::ai::auto_queue(&conn,id,"book")?;db::book_detail(&conn, id)?},
            None => None,
        }
    };

    Ok(Json(json!({
        "capture": capture,
        "book": detail,
        "created": created,
    })))
}

/* ---------------------------------- 图书 ---------------------------------- */

#[derive(Deserialize)]
struct BooksQuery {
    q: Option<String>,
    status: Option<String>,
    limit: Option<i64>,
}

async fn books_list(
    State(state): State<AppState>,
    Query(query): Query<BooksQuery>,
) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    let books = db::list_books(
        &conn,
        query.q.as_deref().unwrap_or(""),
        query.status.as_deref().unwrap_or(""),
        query.limit.unwrap_or(300),
    )?;
    Ok(Json(json!(books)))
}

async fn books_create(
    State(state): State<AppState>,
    Json(input): Json<BookPatch>,
) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    let book = db::create_book(&conn, &input)?;
    let detail = db::book_detail(&conn, book.id)?;
    Ok(Json(json!(detail)))
}

async fn book_get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    let detail = db::book_detail(&conn, id)?.ok_or_else(|| ApiError::not_found("图书不存在"))?;
    Ok(Json(json!(detail)))
}

async fn book_patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<BookPatch>,
) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    db::update_book(&conn, id, &input)?.ok_or_else(|| ApiError::not_found("图书不存在"))?;
    let detail = db::book_detail(&conn, id)?.ok_or_else(|| ApiError::not_found("图书不存在"))?;
    Ok(Json(json!(detail)))
}

async fn book_delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let result = crate::book_deletion::delete(&state, &[id])?;
    if let Some(blocked) = result["skipped"].as_array().and_then(|v| v.first()) {
        return Err(ApiError::bad(blocked["reason"].as_str().unwrap_or("不能删除")));
    }
    Ok(Json(json!({"deleted":true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteBooks { ids: Vec<i64> }

async fn books_delete_preview(State(state): State<AppState>, Json(input): Json<DeleteBooks>) -> Result<Json<Value>, ApiError> {
    let ids = crate::book_deletion::ids(input.ids)?;
    Ok(Json(crate::book_deletion::preview(&state, &ids)?))
}

async fn books_delete(State(state): State<AppState>, Json(input): Json<DeleteBooks>) -> Result<Json<Value>, ApiError> {
    let ids = crate::book_deletion::ids(input.ids)?;
    Ok(Json(crate::book_deletion::delete(&state, &ids)?))
}

/* ---------------------------------- 图片 ---------------------------------- */

#[derive(Deserialize)]
struct SourceQuery {
    source: Option<String>,
}

fn referer_for(source: &str) -> &'static str {
    match source {
        "taobao" => "https://item.taobao.com/",
        "kongfz" => "https://www.kongfz.com/",
        _ => "",
    }
}

fn image_url_exists(
    conn: &rusqlite::Connection,
    book_id: i64,
    url: &str,
) -> rusqlite::Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM images WHERE book_id = ? AND origin_url = ?",
            params![book_id, url],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

async fn images_import(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<ImageImportInput>,
) -> Result<Json<Value>, ApiError> {
    {
        let conn = lock(&state);
        db::get_book(&conn, id)?.ok_or_else(|| ApiError::not_found("图书不存在"))?;
    }
    let source = input
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("kongfz")
        .to_string();
    let referer = referer_for(&source);

    let mut imported: Vec<ImageItem> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();

    for url in input
        .urls
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .take(12)
    {
        let exists = {
            let conn = lock(&state);
            image_url_exists(&conn, id, url)?
        };
        if exists {
            continue;
        }
        match images::download(url, referer, &state.image_dir).await {
            Ok(saved) => {
                let conn = lock(&state);
                match db::add_image(
                    &conn,
                    id,
                    &source,
                    url,
                    &saved.file,
                    saved.width,
                    saved.height,
                ) {
                    Ok(item) => imported.push(item),
                    Err(error) => {
                        images::remove_file(&state.image_dir, &saved.file);
                        errors.push(json!({ "url": url, "error": error.to_string() }));
                    }
                }
            }
            Err(error) => errors.push(json!({ "url": url, "error": format!("{error:#}") })),
        }
    }

    Ok(Json(json!({ "imported": imported, "errors": errors })))
}

async fn image_upload(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<SourceQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    {
        let conn = lock(&state);
        db::get_book(&conn, id)?.ok_or_else(|| ApiError::not_found("图书不存在"))?;
    }
    if body.is_empty() {
        return Err(ApiError::bad("图片内容为空"));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/png");
    let saved = images::save_bytes(&body, content_type, &state.image_dir, "u")?;
    let source = query.source.as_deref().unwrap_or("upload");
    let conn = lock(&state);
    let item = db::add_image(
        &conn,
        id,
        source,
        "",
        &saved.file,
        saved.width,
        saved.height,
    )?;
    Ok(Json(json!(item)))
}

async fn image_file(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let row = {
        let conn = lock(&state);
        db::get_image_row(&conn, id)?
    }
    .ok_or_else(|| ApiError::not_found("图片不存在"))?;

    let want_processed = query
        .get("variant")
        .map(|value| value == "processed")
        .unwrap_or(false);
    let file = if want_processed && !row.processed_file.is_empty() {
        row.processed_file.clone()
    } else {
        row.origin_file.clone()
    };
    if file.is_empty() {
        return Err(ApiError::not_found("图片文件不存在"));
    }
    let name = FsPath::new(&file)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let bytes = std::fs::read(state.image_dir.join(&name))
        .map_err(|_| ApiError::not_found("图片文件不存在"))?;

    let mime = images::mime_for(&name);
    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(response)
}

async fn image_processed(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    if body.is_empty() {
        return Err(ApiError::bad("图片内容为空"));
    }
    let old = {
        let conn = lock(&state);
        db::get_image_row(&conn, id)?.ok_or_else(|| ApiError::not_found("图片不存在"))?
    };
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/png");
    let saved = images::save_bytes(&body, content_type, &state.image_dir, "p")?;
    images::remove_file(&state.image_dir, &old.processed_file);
    let conn = lock(&state);
    let item = db::set_processed(&conn, id, &saved.file, saved.width, saved.height)?
        .ok_or_else(|| ApiError::not_found("图片不存在"))?;
    Ok(Json(json!(item)))
}

async fn image_primary(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    let item = db::set_primary(&conn, id)?.ok_or_else(|| ApiError::not_found("图片不存在"))?;
    Ok(Json(json!(item)))
}

async fn image_delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let row = {
        let conn = lock(&state);
        db::delete_image_row(&conn, id)?
    };
    match row {
        Some(row) => {
            images::remove_file(&state.image_dir, &row.origin_file);
            images::remove_file(&state.image_dir, &row.processed_file);
            Ok(Json(json!({ "deleted": true })))
        }
        None => Err(ApiError::not_found("图片不存在")),
    }
}

#[derive(Deserialize)]
struct OrderInput {
    ids: Vec<i64>,
}

async fn images_order(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<OrderInput>,
) -> Result<Json<Value>, ApiError> {
    let conn = lock(&state);
    db::reorder_images(&conn, id, &input.ids)?;
    Ok(Json(json!({ "ok": true })))
}

/* -------------------------------- 找图上下文 ------------------------------- */

#[derive(Deserialize)]
struct ImportContextInput {
    #[serde(rename = "bookId")]
    book_id: i64,
}

async fn import_get(State(state): State<AppState>) -> Json<Value> {
    let guard = state
        .import_ctx
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_ref() {
        Some(context) if context.created.elapsed().as_secs() < 1800 => Json(json!({
            "active": true,
            "bookId": context.book_id,
            "title": context.title,
            "isbn": context.isbn,
            "ageSeconds": context.created.elapsed().as_secs(),
        })),
        _ => Json(json!({ "active": false })),
    }
}

async fn import_set(
    State(state): State<AppState>,
    Json(input): Json<ImportContextInput>,
) -> Result<Json<Value>, ApiError> {
    let book = {
        let conn = lock(&state);
        db::get_book(&conn, input.book_id)?.ok_or_else(|| ApiError::not_found("图书不存在"))?
    };
    let context = ImportContext {
        book_id: book.id,
        title: book.title,
        isbn: book.isbn,
        created: std::time::Instant::now(),
    };
    let payload = json!({
        "active": true,
        "bookId": context.book_id,
        "title": context.title,
        "isbn": context.isbn,
    });
    *state
        .import_ctx
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(context);
    Ok(Json(payload))
}

async fn import_clear(State(state): State<AppState>) -> Json<Value> {
    *state
        .import_ctx
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    Json(json!({ "active": false }))
}

/* --------------------------------- 发布 ---------------------------------- */

async fn listing_create(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<ListingInput>,
) -> Result<Json<Value>, ApiError> {
    let (book, book_images, settings, input) = {
        let conn = lock(&state);
        (
            db::get_book(&conn, id)?.ok_or_else(|| ApiError::not_found("图书不存在"))?,
            db::images_for_book(&conn, id)?,
            db::settings(&conn),
            crate::agents::ai::listing_defaults(&conn,id,input)?,
        )
    };
    let (built, image_payload) =
        listing::build(&book, &settings, &book_images, &input, &state.base_url());
    if image_payload.is_empty() {
        return Err(ApiError::bad("至少需要一张图片才能发布"));
    }
    let saved = {
        let conn = lock(&state);
        db::save_listing(
            &conn,
            book.id,
            &built.title,
            &built.description,
            Some(built.price),
        )?
    };
    let url = listing::publish_url(&settings.publish_url, saved.id);

    Ok(Json(json!({
        "listing": saved,
        "images": image_payload,
        "publishUrl": url,
    })))
}

async fn listing_payload(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let (listing, book, book_images, settings) = {
        let conn = lock(&state);
        let listing =
            db::get_listing(&conn, id)?.ok_or_else(|| ApiError::not_found("发布任务不存在"))?;
        let book = db::get_book(&conn, listing.book_id)?
            .ok_or_else(|| ApiError::not_found("图书不存在"))?;
        let book_images = db::images_for_book(&conn, listing.book_id)?;
        (listing, book, book_images, db::settings(&conn))
    };

    let input = ListingInput {
        title: None,
        description: None,
        price: listing.price,
        condition: None,
    };
    let (built, image_payload) =
        listing::build(&book, &settings, &book_images, &input, &state.base_url());

    Ok(Json(json!({
        "listingId": listing.id,
        "bookId": book.id,
        "title": if listing.title.is_empty() { built.title } else { listing.title.clone() },
        "description": if listing.description.is_empty() { built.description } else { listing.description.clone() },
        "price": listing.price.unwrap_or(built.price),
        "condition": book.condition,
        "autoSubmit": settings.auto_submit,
        "images": image_payload,
        "publishUrl": listing::publish_url(&settings.publish_url, listing.id),
    })))
}

async fn listing_report(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<ListingReport>,
) -> Result<Json<Value>, ApiError> {
    let status = input.status.trim();
    if !["draft", "filling", "published", "failed"].contains(&status) {
        return Err(ApiError::bad("未知的发布状态"));
    }
    let conn = lock(&state);
    let listing = db::set_listing_status(&conn, id, status, &input.url, &input.error)?
        .ok_or_else(|| ApiError::not_found("发布任务不存在"))?;
    Ok(Json(json!(listing)))
}

/* --------------------------------- 导出 ---------------------------------- */

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

async fn export_csv(State(state): State<AppState>) -> Result<Response, ApiError> {
    let books = {
        let conn = lock(&state);
        db::list_books(&conn, "", "", 1000)?
    };
    let mut csv =
        String::from("\u{feff}ID,书名,作者,出版社,ISBN,品相,成本价,售价,状态,图片数,创建时间\n");
    for book in books {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{}\n",
            book.id,
            csv_field(&book.title),
            csv_field(&book.author),
            csv_field(&book.publisher),
            csv_field(&book.isbn),
            csv_field(&book.condition),
            book.cost_price
                .map(|v| format!("{v:.2}"))
                .unwrap_or_default(),
            book.sell_price
                .map(|v| format!("{v:.2}"))
                .unwrap_or_default(),
            csv_field(&book.status),
            book.image_count,
            csv_field(&book.created_at),
        ));
    }
    let mut response = Response::new(Body::from(csv));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"taoshu-books.csv\""),
    );
    Ok(response)
}
