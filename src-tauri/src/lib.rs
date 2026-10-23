pub mod api;
pub mod commerce;
pub mod db;
pub mod images;
pub mod gitee;
pub mod listing;
pub mod models;
pub mod regions;
pub mod state;
pub mod xianyu;

use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::state::AppState;

fn bridge_check(state: &AppState, secret: &str) -> Result<(), String> {
    if secret != state.bridge_secret {
        return Err("非法的桥接密钥".into());
    }
    Ok(())
}

#[tauri::command]
async fn bridge_fetch(
    state: State<'_, AppState>,
    secret: String,
    path: String,
    method: String,
    body: Option<Value>,
) -> Result<Value, String> {
    bridge_check(&state, &secret)?;
    if !path.starts_with('/') {
        return Err("非法路径".into());
    }
    let client = reqwest::Client::new();
    let url = format!("{}{}", state.base_url(), path);
    let mut request = match method.to_uppercase().as_str() {
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "PATCH" => client.patch(&url),
        "DELETE" => client.delete(&url),
        _ => client.get(&url),
    };
    request = request.header("x-taoshu-secret", &state.bridge_secret);
    if let Some(payload) = body {
        request = request.json(&payload);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    Ok(json!({
        "status": status,
        "contentType": content_type,
        "bodyBase64": STANDARD.encode(bytes),
    }))
}

#[tauri::command]
fn focus_main(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    secret: String,
) -> Result<(), String> {
    bridge_check(&state, &secret)?;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}

#[tauri::command]
async fn open_browser(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    kind: String,
    url: Option<String>,
) -> Result<String, String> {
    let settings = {
        let conn = state::lock(&state);
        db::settings(&conn)
    };

    let (label, title, default_url): (&str, &str, String) = match kind.as_str() {
        "taobao" => (
            "browser-taobao",
            "淘宝 · 淘书工坊",
            "https://www.taobao.com/".into(),
        ),
        "kongfz" => (
            "browser-kongfz",
            "孔夫子旧书网 · 淘书工坊",
            "https://www.kongfz.com/".into(),
        ),
        "goofish" => (
            "browser-goofish",
            "闲鱼卖家中心 · 淘书工坊",
            settings.publish_url.clone(),
        ),
        other => return Err(format!("未知的窗口类型: {other}")),
    };

    let target = url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(default_url);

    if let Some(window) = app.get_webview_window(label) {
        let parsed = tauri::Url::parse(&target).map_err(|error| error.to_string())?;
        window.navigate(parsed).map_err(|error| error.to_string())?;
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(label.to_string());
    }

    let config = json!({
        "apiBase": state.base_url(),
        "secret": state.bridge_secret,
        "host": "tauri",
        "kind": kind,
    });
    let init_script = format!(
        "window.__TAOSHU_CONFIG__ = {};\n{}",
        serde_json::to_string(&config).unwrap_or_else(|_| "{}".into()),
        include_str!("../inject/browser.js")
    );

    let parsed = tauri::Url::parse(&target).map_err(|error| error.to_string())?;
    let builder = WebviewWindowBuilder::new(&app, label, WebviewUrl::External(parsed))
        .title(title)
        .inner_size(1280.0, 880.0)
        .min_inner_size(900.0, 600.0)
        .initialization_script(init_script)
        .on_navigation(|url| matches!(url.scheme(), "http" | "https" | "about"));
    builder.build().map_err(|error| error.to_string())?;

    Ok(label.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("./data"));
            let port = std::env::var("PORT")
                .ok()
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(8899);
            let state = state::AppState::new(data_dir, port)?;
            app.manage(state.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(error) = api::serve(state).await {
                    eprintln!("[api] 本地接口启动失败: {error:#}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_browser,
            bridge_fetch,
            focus_main
        ])
        .run(tauri::generate_context!())
        .expect("运行淘书工坊失败");
}
