//! Public image hosting through Gitee's documented repository contents API.
use crate::{
    api::ApiError,
    db, images,
    models::ImageRow,
    state::{lock, AppState},
};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostingConfig {
    repo_url: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    branch: String,
    #[serde(default = "default_directory")]
    directory: String,
}
fn default_directory() -> String {
    "taoshu-images".into()
}
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/xianyu/gitee/config", get(config_get).put(config_put))
        .route("/api/xianyu/gitee/test", post(test))
}
fn repository(url: &str) -> Result<(String, String), ApiError> {
    let url = Url::parse(url.trim()).map_err(|_| ApiError::bad("请填写完整 Gitee 仓库链接"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("gitee.com")
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ApiError::bad("仓库链接须为 https://gitee.com/所有者/仓库"));
    }
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    if parts.len() != 2 {
        return Err(ApiError::bad("请填写仓库首页链接，不含文件路径"));
    }
    let owner = parts[0];
    let repo = parts[1].trim_end_matches(".git");
    if [owner, repo].iter().any(|s| {
        s.is_empty()
            || !s
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            || *s == ".."
    }) {
        return Err(ApiError::bad("仓库所有者或名称无效"));
    }
    Ok((owner.into(), repo.into()))
}
fn validate(config: &HostingConfig) -> Result<(), ApiError> {
    repository(&config.repo_url)?;
    if config.directory.is_empty()
        || config.directory.split('/').any(|s| {
            s.is_empty()
                || s == "."
                || s == ".."
                || !s
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        })
    {
        return Err(ApiError::bad(
            "图片目录只能使用字母、数字、短横线、下划线和分隔斜线",
        ));
    }
    if config.branch.chars().any(char::is_control) {
        return Err(ApiError::bad("分支名称无效"));
    }
    Ok(())
}
fn config(state: &AppState) -> Result<HostingConfig, ApiError> {
    let conn = lock(state);
    let raw = db::get_setting(&conn, "xianyuGitee")
        .ok_or_else(|| ApiError::bad("请先配置 Gitee 图片仓库"))?;
    serde_json::from_value(raw).map_err(ApiError::from)
}
async fn config_get(State(state): State<AppState>) -> Json<Value> {
    match config(&state) {
        Ok(c) => Json(
            json!({"repoUrl":c.repo_url,"branch":c.branch,"directory":c.directory,"tokenConfigured":!c.token.is_empty()}),
        ),
        Err(_) => Json(
            json!({"repoUrl":"","branch":"","directory":default_directory(),"tokenConfigured":false}),
        ),
    }
}
async fn config_put(
    State(state): State<AppState>,
    Json(mut c): Json<HostingConfig>,
) -> Result<Json<Value>, ApiError> {
    c.repo_url = c.repo_url.trim().trim_end_matches('/').into();
    c.branch = c.branch.trim().into();
    c.directory = c.directory.trim().trim_matches('/').into();
    validate(&c)?;
    if c.token.trim().is_empty() {
        c.token = config(&state).map(|old| old.token).unwrap_or_default();
    } else {
        c.token = c.token.trim().into();
    }
    if c.token.is_empty() {
        return Err(ApiError::bad("请填写 Gitee Token"));
    }
    db::set_setting(
        &lock(&state),
        "xianyuGitee",
        &json!({"repoUrl":c.repo_url,"token":c.token,"branch":c.branch,"directory":c.directory}),
    )?;
    Ok(config_get(State(state)).await)
}
fn client() -> Result<Client, ApiError> {
    // Authenticated API calls never follow redirects with credentials.
    Client::builder()
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ApiError::bad("无法初始化 Gitee 连接"))
}
fn endpoint(base: &str, owner: &str, repo: &str, segments: &[&str]) -> Url {
    let mut url = Url::parse(base).expect("constant URL");
    url.path_segments_mut()
        .unwrap()
        .extend(["api", "v5", "repos", owner, repo])
        .extend(segments);
    url
}
async fn repo_info(client: &Client, c: &HostingConfig, base: &str) -> Result<Value, ApiError> {
    let (owner, repo) = repository(&c.repo_url)?;
    let response = client
        .get(endpoint(base, &owner, &repo, &[]))
        .query(&[("access_token", c.token.as_str())])
        .send()
        .await
        .map_err(|_| ApiError::bad("Gitee 连接失败，请检查网络后重试"))?;
    if !response.status().is_success() {
        return Err(ApiError::bad(format!(
            "无法读取 Gitee 仓库（HTTP {}），请检查仓库链接和 Token 权限",
            response.status()
        )));
    }
    let data: Value = response
        .json()
        .await
        .map_err(|_| ApiError::bad("Gitee 返回了非 JSON 响应"))?;
    if data["private"] != false || data["public"] == false {
        return Err(ApiError::bad(
            "图片仓库必须公开，闲管家才能匿名读取；请在 Gitee 确认仓库可见性",
        ));
    }
    Ok(data)
}
async fn test(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let c = config(&state)?;
    validate(&c)?;
    let info = repo_info(&client()?, &c, "https://gitee.com").await?;
    Ok(Json(
        json!({"ok":true,"defaultBranch":info["default_branch"],"fullName":info["full_name"],"message":"公开仓库可读取；上传权限和图片匿名访问会在发布时进一步验证"}),
    ))
}

pub async fn upload_book(
    state: &AppState,
    book_id: i64,
    rows: &[ImageRow],
    first_only: bool,
) -> Result<Vec<String>, ApiError> {
    let c = config(state)?;
    validate(&c)?;
    let client = client()?;
    let info = repo_info(&client, &c, "https://gitee.com").await?;
    let branch = if c.branch.is_empty() {
        info["default_branch"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::bad("仓库没有默认分支，请先初始化仓库或填写分支"))?
            .to_string()
    } else {
        c.branch.clone()
    };
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_by_key(|r| (-r.is_primary, r.sort, r.id));
    let mut urls = Vec::new();
    let mut total = 0;
    for row in sorted.into_iter().take(if first_only { 1 } else { 9 }) {
        let file = if row.processed_file.is_empty() {
            &row.origin_file
        } else {
            &row.processed_file
        };
        let name = Path::new(file)
            .file_name()
            .ok_or_else(|| ApiError::bad("图片文件名无效"))?;
        let bytes = std::fs::read(state.image_dir.join(name))?;
        total += bytes.len();
        if total > 20 * 1024 * 1024 {
            return Err(ApiError::bad("图片合计超过 20MB，请压缩后重试"));
        }
        let mime = images::mime_for(&name.to_string_lossy());
        let extension = match mime {
            "image/png" => "png",
            "image/webp" => "webp",
            "image/gif" => "gif",
            "image/bmp" => "bmp",
            _ => "jpg",
        };
        let path = format!(
            "{}/{book_id}/{:x}.{extension}",
            c.directory,
            md5::compute(&bytes)
        );
        let url = upload(&client, &c, "https://gitee.com", &branch, &path, &bytes).await?;
        urls.push(url);
    }
    Ok(urls)
}
async fn upload(
    client: &Client,
    c: &HostingConfig,
    base: &str,
    branch: &str,
    path: &str,
    bytes: &[u8],
) -> Result<String, ApiError> {
    let (owner, repo) = repository(&c.repo_url)?;
    let path_segments: Vec<_> = std::iter::once("contents").chain(path.split('/')).collect();
    let api_url = endpoint(base, &owner, &repo, &path_segments);
    let existing = client
        .get(api_url.clone())
        .query(&[("access_token", c.token.as_str()), ("ref", branch)])
        .send()
        .await
        .map_err(|_| ApiError::bad("检查 Gitee 图片文件失败，请重试；相同图片会复用已有文件"))?;
    let mut metadata = if existing.status() == reqwest::StatusCode::NOT_FOUND {
        None
    } else if existing.status().is_success() {
        let data: Value = existing
            .json()
            .await
            .map_err(|_| ApiError::bad("Gitee 文件查询返回了非 JSON 响应，未继续上传"))?;
        file_metadata(&data)?.cloned()
    } else {
        return Err(ApiError::bad(format!(
            "无法检查 Gitee 文件（HTTP {}）",
            existing.status()
        )));
    };
    // Gitee also returns HTTP 200 + [] for a missing file. A successful status
    // alone does not mean that the image has already been uploaded.
    if metadata.is_none() {
        let response=client.post(api_url).json(&json!({"access_token":c.token,"content":STANDARD.encode(bytes),"message":"Upload Taoshu book image","branch":branch})).send().await.map_err(|_|ApiError::bad("Gitee 上传连接失败或超时；请重试，相同图片不会重复提交"))?;
        if response.status() != reqwest::StatusCode::CREATED {
            return Err(ApiError::bad(format!("Gitee 上传失败（HTTP {}），请检查 Token 的仓库写入权限、分支及文件大小；已有文件会在下次重试时复用",response.status())));
        }
        let uploaded: Value = response.json().await.map_err(|_| {
            ApiError::bad("Gitee 已接收上传，但返回内容无法解析；可重试复用已上传的图片")
        })?;
        metadata = uploaded.get("content").cloned();
    }
    let raw = image_url(base, &owner, &repo, branch, path, metadata.as_ref())?;
    // This client has no cookies, Authorization or token; test the exact URL given to XianGuanJia.
    let anonymous = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|_| ApiError::bad("无法验证图片直链"))?;
    let mut response =
        anonymous.get(raw.clone()).send().await.map_err(|_| {
            ApiError::bad("Gitee 图片已上传，但匿名读取失败；请检查公开仓库的直链访问")
        })?;
    // A just-created file can briefly lag behind the contents API on the raw service.
    for delay in [350, 1000] {
        if response.status() != reqwest::StatusCode::NOT_FOUND {
            break;
        }
        tokio::time::sleep(Duration::from_millis(delay)).await;
        response = anonymous
            .get(raw.clone())
            .send()
            .await
            .map_err(|_| ApiError::bad("Gitee 图片直链重试失败，请稍后重试"))?;
    }
    if !response.status().is_success() {
        return Err(ApiError::bad(format!(
            "Gitee 图片已上传，但匿名访问失败（HTTP {}）。直链：{}；未继续提交闲管家，可稍后重试",
            response.status(),
            raw
        )));
    }
    if response
        .content_length()
        .map(|n| n > 20 * 1024 * 1024)
        .unwrap_or(false)
    {
        return Err(ApiError::bad("Gitee 图片响应大小异常"));
    }
    let downloaded = response
        .bytes()
        .await
        .map_err(|_| ApiError::bad("无法读取 Gitee 图片直链响应"))?;
    if image::guess_format(&downloaded).is_err() || md5::compute(&downloaded) != md5::compute(bytes)
    {
        return Err(ApiError::bad(
            "Gitee 直链未返回上传的原始图片，可能是登录页或访问拦截；未继续提交闲管家",
        ));
    }
    Ok(raw.to_string())
}
fn file_metadata(data: &Value) -> Result<Option<&Value>, ApiError> {
    match data {
        Value::Array(items) if items.is_empty() => Ok(None),
        Value::Object(_) if data["type"] == "file" => Ok(Some(data)),
        Value::Array(_) => Err(ApiError::bad(
            "Gitee 图片路径指向目录，请更换图片目录后重试",
        )),
        _ => Err(ApiError::bad(
            "Gitee 文件查询未返回有效的文件信息，未继续上传",
        )),
    }
}
fn image_url(
    base: &str,
    owner: &str,
    repo: &str,
    branch: &str,
    path: &str,
    metadata: Option<&Value>,
) -> Result<Url, ApiError> {
    let Some(download) = metadata
        .and_then(|m| m["download_url"].as_str())
        .filter(|s| !s.is_empty())
    else {
        return Ok(raw_url(base, owner, repo, branch, path));
    };
    let url = Url::parse(download).map_err(|_| ApiError::bad("Gitee 返回的图片下载地址无效"))?;
    let origin = Url::parse(base).expect("constant URL");
    if url.origin() != origin.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with(&format!("/{owner}/{repo}/raw/"))
    {
        return Err(ApiError::bad("Gitee 返回的图片下载地址不属于当前公开仓库"));
    }
    Ok(url)
}
fn raw_url(base: &str, owner: &str, repo: &str, branch: &str, path: &str) -> Url {
    let mut url = Url::parse(base).expect("constant URL");
    url.path_segments_mut()
        .unwrap()
        .extend([owner, repo, "raw", branch])
        .extend(path.split('/'));
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::Request,
        http::{Method, StatusCode},
        response::Response,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };
    #[derive(Clone)]
    struct Mock {
        bytes: Vec<u8>,
        existing: bool,
        html: bool,
        missing_as_array: bool,
        raw_misses: Arc<Mutex<usize>>,
        raw_requests: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<(String, Value)>>>,
    }
    async fn handler(State(m): State<Mock>, request: Request) -> Response {
        let path = request.uri().path().to_string();
        let method = request.method().clone();
        if path.contains("/raw/") {
            assert!(request.uri().query().is_none());
            assert!(!request.headers().contains_key("authorization"));
            m.raw_requests.fetch_add(1, Ordering::SeqCst);
            let missing = {
                let mut remaining = m.raw_misses.lock().unwrap();
                if *remaining > 0 {
                    *remaining -= 1;
                    true
                } else {
                    false
                }
            };
            if missing {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap();
            }
            return Response::new(Body::from(if m.html {
                b"<html>sign in</html>".to_vec()
            } else {
                m.bytes
            }));
        }
        let host = request.headers().get("host").unwrap().to_str().unwrap();
        let metadata = json!({"type":"file","path":"images/test.png","download_url":format!("http://{host}/owner/images/raw/api-returned-ref/images/test.png")});
        if method == Method::GET {
            if m.existing || m.missing_as_array {
                return Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(if m.existing {
                        metadata.to_string()
                    } else {
                        "[]".into()
                    }))
                    .unwrap();
            }
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap();
        }
        let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        m.requests.lock().unwrap().push((path, body));
        Response::builder()
            .status(StatusCode::CREATED)
            .header("content-type", "application/json")
            .body(Body::from(json!({"content":metadata}).to_string()))
            .unwrap()
    }
    fn c() -> HostingConfig {
        HostingConfig {
            repo_url: "https://gitee.com/owner/images".into(),
            token: "local-token".into(),
            branch: "main".into(),
            directory: "taoshu-images".into(),
        }
    }
    #[test]
    fn repository_validation_and_raw_links() {
        assert_eq!(
            repository("https://gitee.com/owner/images.git").unwrap(),
            ("owner".into(), "images".into())
        );
        assert!(repository("https://evil.example/owner/images").is_err());
        assert!(repository("https://gitee.com/owner/images/blob/main").is_err());
        let mut cfg = c();
        cfg.directory = "../images".into();
        assert!(validate(&cfg).is_err());
        let url = raw_url(
            "https://gitee.com",
            "owner",
            "images",
            "main",
            "taoshu-images/1/hash.png",
        );
        assert_eq!(
            url.as_str(),
            "https://gitee.com/owner/images/raw/main/taoshu-images/1/hash.png"
        );
        assert!(url.query().is_none());
    }
    async fn mock(existing: bool, html: bool) -> (String, Mock, tokio::task::JoinHandle<()>) {
        mock_behavior(existing, html, false, 0).await
    }
    async fn mock_behavior(
        existing: bool,
        html: bool,
        missing_as_array: bool,
        raw_misses: usize,
    ) -> (String, Mock, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let bytes=STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a1ioAAAAASUVORK5CYII=").unwrap();
        let m = Mock {
            bytes,
            existing,
            html,
            missing_as_array,
            raw_misses: Arc::new(Mutex::new(raw_misses)),
            raw_requests: Arc::new(AtomicUsize::new(0)),
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let router = Router::new().fallback(handler).with_state(m.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (base, m, server)
    }
    #[tokio::test]
    async fn uploads_base64_and_checks_public_raw_image_without_token() {
        let (base, m, server) = mock(false, false).await;
        let url = upload(
            &client().unwrap(),
            &c(),
            &base,
            "main",
            "images/test.png",
            &m.bytes,
        )
        .await
        .unwrap();
        assert!(url.ends_with("/owner/images/raw/api-returned-ref/images/test.png"));
        assert!(!url.contains("local-token"));
        let req = m.requests.lock().unwrap();
        assert_eq!(req.len(), 1);
        assert_eq!(req[0].1["access_token"], "local-token");
        assert_eq!(req[0].1["content"], STANDARD.encode(&m.bytes));
        server.abort();
    }
    #[tokio::test]
    async fn reuses_existing_content_without_new_commit() {
        let (base, m, server) = mock(true, false).await;
        upload(
            &client().unwrap(),
            &c(),
            &base,
            "main",
            "images/test.png",
            &m.bytes,
        )
        .await
        .unwrap();
        assert!(m.requests.lock().unwrap().is_empty());
        server.abort();
    }
    #[tokio::test]
    async fn rejects_html_instead_of_passing_login_url_to_xianyu() {
        let (base, m, server) = mock(false, true).await;
        assert!(upload(
            &client().unwrap(),
            &c(),
            &base,
            "main",
            "images/test.png",
            &m.bytes
        )
        .await
        .is_err());
        server.abort();
    }
    #[tokio::test]
    async fn empty_array_means_missing_file_and_must_upload() {
        let (base, m, server) = mock_behavior(false, false, true, 0).await;
        upload(
            &client().unwrap(),
            &c(),
            &base,
            "main",
            "images/test.png",
            &m.bytes,
        )
        .await
        .unwrap();
        assert_eq!(m.requests.lock().unwrap().len(), 1);
        server.abort();
    }
    #[tokio::test]
    async fn retries_raw_visibility_without_uploading_another_commit() {
        let (base, m, server) = mock_behavior(false, false, true, 1).await;
        upload(
            &client().unwrap(),
            &c(),
            &base,
            "main",
            "images/test.png",
            &m.bytes,
        )
        .await
        .unwrap();
        assert_eq!(m.requests.lock().unwrap().len(), 1);
        assert_eq!(m.raw_requests.load(Ordering::SeqCst), 2);
        server.abort();
    }
    #[tokio::test]
    async fn persistent_raw_404_is_reported_without_resubmitting_upload() {
        let (base, m, server) = mock_behavior(false, false, true, 9).await;
        let error = upload(
            &client().unwrap(),
            &c(),
            &base,
            "main",
            "images/test.png",
            &m.bytes,
        )
        .await
        .unwrap_err();
        assert_eq!(m.requests.lock().unwrap().len(), 1);
        assert_eq!(m.raw_requests.load(Ordering::SeqCst), 3);
        let response = axum::response::IntoResponse::into_response(error);
        let bytes = axum::body::to_bytes(response.into_body(), 10000)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("404"));
        assert!(!text.contains("local-token"));
        server.abort();
    }
    #[test]
    fn rejects_directory_or_credential_bearing_download_links() {
        assert!(file_metadata(&json!([])).unwrap().is_none());
        assert!(file_metadata(&json!([{"type":"file"}])).is_err());
        assert!(file_metadata(&json!({"message":"error"})).is_err());
        for url in [
            "https://gitee.com/owner/images/raw/main/test.png?access_token=secret",
            "https://evil.example/test.png",
        ] {
            assert!(image_url(
                "https://gitee.com",
                "owner",
                "images",
                "main",
                "test.png",
                Some(&json!({"download_url":url}))
            )
            .is_err());
        }
    }
}
