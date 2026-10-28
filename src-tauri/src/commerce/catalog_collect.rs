//! Runtime-editable kaoyan collection (P1).
//! Rust is only the host + a stable downstream: each school's collector is an external `.mjs`
//! plugin run by spawning the system `node`, so adapters can be dropped or edited from the UI
//! without repackaging the app. The heavy normalize/dedupe + catalog.db build reuses
//! `scripts/kaoyan/build-catalog.mjs`. `command` args are passed as a vector (never through a
//! shell), so user-supplied URLs/text cannot be injected.
use super::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MAX_PLUGIN_BYTES: usize = 200_000;

// P1 is repo-relative: scripts live in the source tree and `node` runs with the repo as cwd so
// the scripts' `data/kaoyan/...` paths resolve. This does not work in a packaged bundle.
fn repo_root() -> Result<PathBuf, ApiError> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.to_path_buf())
        .ok_or_else(|| ApiError::bad("无法定位项目根目录"))
}

fn scripts_dir() -> Result<PathBuf, ApiError> {
    Ok(super::jobs::script_root())
}

fn user_adapter_dir() -> Result<PathBuf, ApiError> {
    Ok(repo_root()?.join("data").join("kaoyan").join("adapters"))
}

fn tail(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    if count <= max {
        trimmed.to_string()
    } else {
        trimmed.chars().skip(count - max).collect()
    }
}

fn node_error(error: std::io::Error) -> ApiError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ApiError::bad("未找到 node，请安装 Node.js 并加入 PATH 后重试")
    } else {
        ApiError::bad(format!("调用 node 失败：{error}"))
    }
}

fn decode_output(output: &Output, script: &Path) -> Result<Value, ApiError> {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        let detail = tail(&stderr, 1200);
        return Err(ApiError::bad(format!(
            "{} 运行失败：{}",
            script
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("脚本"),
            if detail.is_empty() {
                format!("退出码 {:?}", output.status.code())
            } else {
                detail
            }
        )));
    }
    serde_json::from_str(&stdout).map_err(|_| {
        ApiError::bad(format!(
            "{} 输出解析失败：{}",
            script.display(),
            tail(&stdout, 400)
        ))
    })
}

fn run_node(script: PathBuf, args: Vec<String>) -> Result<Value, ApiError> {
    let root = std::env::temp_dir();
    let output = Command::new(super::jobs::node())
        .arg(&script)
        .args(&args)
        .current_dir(&root)
        .output()
        .map_err(node_error)?;
    decode_output(&output, &script)
}

async fn spawn_node(script: PathBuf, args: Vec<String>) -> Result<Value, ApiError> {
    tokio::task::spawn_blocking(move || run_node(script, args))
        .await
        .map_err(|error| ApiError::bad(format!("采集任务执行失败：{error}")))?
}

// GET /api/commerce/catalog/adapters — list builtin + user plugins with their meta.fields.
pub(super) async fn adapters(State(_state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let script = scripts_dir()?.join("list-adapters.mjs");
    Ok(Json(spawn_node(script, Vec::new()).await?))
}

fn safe_adapter_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(ApiError::bad("请填写插件文件名"));
    }
    if Path::new(name)
        .parent()
        .map(|p| !p.as_os_str().is_empty())
        .unwrap_or(false)
    {
        return Err(ApiError::bad("插件文件名不能包含路径"));
    }
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".mjs") {
        return Err(ApiError::bad("插件文件名须以 .mjs 结尾"));
    }
    let stem = &name[..name.len() - 4];
    if stem.is_empty()
        || !stem
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(ApiError::bad("插件名只能包含字母、数字与 - _ ."));
    }
    Ok(name.to_string())
}

// POST /api/commerce/catalog/adapters — { name, content } → write a user plugin, return the list.
pub(super) async fn save_adapter(
    State(state): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let name = safe_adapter_name(text(&v, "name"))?;
    let content = v["content"]
        .as_str()
        .ok_or_else(|| ApiError::bad("缺少插件内容"))?;
    if content.trim().is_empty() {
        return Err(ApiError::bad("插件内容为空"));
    }
    if content.len() > MAX_PLUGIN_BYTES {
        return Err(ApiError::bad("插件过大"));
    }
    let dir = user_adapter_dir()?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(&name), content)?;
    adapters(State(state)).await
}

// POST /api/commerce/catalog/delete-adapter — { name } → remove a user plugin. Builtin plugins are
// part of the repo and cannot be deleted from here; the UI hides the action for them.
pub(super) async fn delete_adapter(
    State(state): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let name = safe_adapter_name(text(&v, "name"))?;
    let path = user_adapter_dir()?.join(&name);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    adapters(State(state)).await
}

fn field_flag(field: &str) -> Option<&'static str> {
    Some(match field {
        "school" => "--school",
        "url" => "--url",
        "slug" => "--slug",
        "title" => "--title",
        "college" => "--college",
        "majorCode" => "--major-code",
        "majorName" => "--major-name",
        _ => return None,
    })
}

// Build the run-plugin.mjs argv. `year/delay/limit/refresh` configure the host; plugin-specific
// fields go through `--set key=value` (the generic path that also covers custom adapters).
pub(super) fn build_collect_args(v: &Value) -> Result<Vec<String>, ApiError> {
    let plugin = text(v, "plugin");
    if plugin.is_empty() {
        return Err(ApiError::bad("请选择采集插件"));
    }
    let mut args = vec!["--plugin".to_string(), plugin.to_string()];
    for (key, flag) in [
        ("year", "--year"),
        ("delay", "--delay"),
        ("limit", "--limit"),
    ] {
        let value = match &v[key] {
            Value::String(text) => text.trim().to_string(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        if !value.is_empty() {
            args.push(flag.to_string());
            args.push(value);
        }
    }
    if v["refresh"].as_bool().unwrap_or(false) {
        args.push("--refresh".to_string());
    }

    let fields = v["args"].as_object().cloned().unwrap_or_default();
    let mut seen = 0usize;
    for (field, value) in &fields {
        let text_value = match value {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => String::new(),
        };
        if text_value.is_empty() {
            continue;
        }
        if let Some(flag) = field_flag(field) {
            args.push(flag.to_string());
            args.push(text_value);
            continue;
        }
        if field == "adapter" {
            continue;
        }
        if !field
            .chars()
            .next()
            .map(char::is_alphabetic)
            .unwrap_or(false)
            || !field.chars().all(|c| c.is_alphanumeric() || c == '_')
        {
            return Err(ApiError::bad(format!("字段名无效：{field}")));
        }
        seen += 1;
        if seen > 64 {
            return Err(ApiError::bad("字段过多"));
        }
        args.push("--set".to_string());
        args.push(format!("{field}={text_value}"));
    }
    Ok(args)
}

// POST /api/commerce/catalog/collect — run a plugin, then rebuild catalog.db when `rebuild`.
pub(super) async fn collect(Json(v): Json<Value>) -> Result<Json<Value>, ApiError> {
    let collect_args = build_collect_args(&v)?;
    let rebuild = v["rebuild"].as_bool().unwrap_or(false);
    let run_script = scripts_dir()?.join("run-plugin.mjs");
    let collect = spawn_node(run_script, collect_args).await?;

    let mut result = json!({ "collect": collect });
    if rebuild {
        let build_script = scripts_dir()?.join("build-catalog.mjs");
        result["rebuild"] = json!(spawn_node(build_script, Vec::new()).await?);
    }
    Ok(Json(result))
}
