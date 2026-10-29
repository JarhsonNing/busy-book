//! Local agent integration: executable selection and scoped MCP imports.
mod imports;
pub mod ai;
mod isbn;
pub mod mcp;
mod opencode;
pub mod qoder;
#[cfg(test)]
mod tests;
pub mod web;
use crate::{
    api::ApiError,
    db,
    state::{lock, AppState},
};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
const SKILL: &str = include_str!("../../../agent-skills/taoshu-import/SKILL.md");
const WORKFLOW_SKILL: &str = include_str!("../../../agent-skills/taoshu-workflow/SKILL.md");
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/mcp",
            post(mcp::post).get(mcp::no_stream).delete(mcp::no_stream),
        )
        .route(
            "/api/agents/opencode",
            get(opencode::status).post(opencode::select),
        )
        .route("/api/agents/opencode/browse", post(opencode::browse))
        .route("/api/agents/opencode/install", post(opencode::install))
        .route("/api/agents/opencode/launch", post(web::start))
        .route("/api/agents/qoder", get(qoder::status).post(qoder::select))
        .route("/api/agents/qoder/browse", post(qoder::browse))
        .route("/api/agents/qoder/install", post(qoder::install))
        .route("/api/agents/qoder/launch", post(qoder::launch))
        .route("/api/agents/qoder/login", post(qoder::login))
        .route("/api/agents/qoder/account", get(qoder::account))
        .route("/api/agents/qoder/cli", get(qoder::cli_status).delete(qoder::stop))
        .route(
            "/api/agents/opencode/web",
            get(web::status).delete(web::stop),
        )
        .route("/api/agents/mcp", get(mcp_status).post(mcp_control))
        .route("/api/agents/mcp/export", post(export))
        .route("/api/agents/books/batch", post(batch_books))
        .route("/api/agents/isbn", post(lookup_isbn))
        .route("/api/agents/ai/config", get(ai::get_config).post(ai::save_config))
        .route("/api/agents/ai/models", get(ai::models))
        .route("/api/agents/ai/command", post(ai::cli_command))
        .route("/api/agents/ai/books/{id}/draft", get(ai::get_draft).post(ai::save_draft))
}
pub fn init(c: &rusqlite::Connection) -> anyhow::Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS agent_isbn_cache(cache_key TEXT PRIMARY KEY,data TEXT NOT NULL,expires_at INTEGER NOT NULL,updated_at TEXT NOT NULL);")?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS agent_imports(id INTEGER PRIMARY KEY AUTOINCREMENT,tool TEXT NOT NULL,source TEXT NOT NULL,external_id TEXT NOT NULL,payload_hash TEXT NOT NULL,result TEXT NOT NULL,source_url TEXT NOT NULL DEFAULT '',created_at TEXT NOT NULL,UNIQUE(tool,source,external_id));")?;
    let has_source = c
        .prepare("PRAGMA table_info(agent_imports)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == "source_url");
    if !has_source {
        c.execute(
            "ALTER TABLE agent_imports ADD COLUMN source_url TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if db::get_setting(c, "agentMcpToken").is_none() {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        db::set_setting(c, "agentMcpToken", &json!(URL_SAFE_NO_PAD.encode(bytes)))?;
    }
    for key in ["agentOpenCodeInstall", "agentQoderInstall"] {
        if let Some(mut job) = db::get_setting(c, key) {
            if job["state"] == "running" {
                job["state"] = json!("failed");
                job["error"] = json!("安装因应用退出而中断，请重试");
                db::set_setting(c, key, &job)?;
            }
        }
    }
    Ok(())
}
async fn batch_books(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    tokio::task::spawn_blocking(move || imports::batch(&s, &v))
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
        .map(Json)
        .map_err(ApiError::bad)
}
async fn lookup_isbn(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    tokio::task::spawn_blocking(move || isbn::lookup(&s, &v))
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
        .map(Json)
        .map_err(ApiError::bad)
}
fn support(s: &AppState) -> PathBuf {
    s.data_dir.join("agent-support")
}
// The application, rather than a request or the process CWD, owns this directory.
fn workspace(s: &AppState) -> Result<PathBuf, ApiError> {
    let dir = support(s).join("workspace");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}
fn node_path() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("TAOSHU_COLLECTOR") {
        let p = PathBuf::from(root).join(if cfg!(windows) { "node.exe" } else { "node" });
        if p.is_file() {
            return Some(p);
        }
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|p| p.join(if cfg!(windows) { "node.exe" } else { "node" }))
        .find(|p| p.is_file())
}
fn write_config(path: &Path, bytes: &[u8]) -> Result<(), ApiError> {
    if fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    let temporary = path.with_extension(format!("tmp-{}", rand::random::<u64>()));
    fs::write(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}
fn configs(s: &AppState) -> Result<Value, ApiError> {
    let root = support(s);
    let bridge = root.join("mcp-stdio.mjs");
    let connection = root.join("connection.json");
    let node =
        node_path().ok_or_else(|| ApiError::bad("未找到内置 Node 运行环境，请使用完整桌面版本"))?;
    Ok(
        json!({"stdio":{"mcpServers":{"taoshu":{"command":node,"args":[bridge,"--config",connection]}}},
        "opencode":{"$schema":"https://opencode.ai/config.json","mcp":{"servers":{"taoshu":{"type":"local","command":[node,bridge,"--config",connection],"codemode":false}}}},
        "qoder":{"mcpServers":{"taoshu":{"command":node,"args":[bridge,"--config",connection]}}},
        "workspace":root.join("workspace"),"skillPath":root.join("workspace/.opencode/skills/taoshu-import/SKILL.md"),"workflowSkillPath":root.join("workspace/.opencode/skills/taoshu-workflow/SKILL.md"),"qoderSkillPath":root.join("workspace/.qoder/skills/taoshu-import/SKILL.md"),"qoderWorkflowSkillPath":root.join("workspace/.qoder/skills/taoshu-workflow/SKILL.md")}),
    )
}
fn prepare(s: &AppState) -> Result<Value, ApiError> {
    let cfg = configs(s)?;
    let root = support(s);
    fs::create_dir_all(&root)?;
    let token = db::get_setting(&lock(s), "agentMcpToken").unwrap();
    write_config(
        &root.join("connection.json"),
        &serde_json::to_vec_pretty(&json!({"url":format!("{}/mcp",s.base_url()),"token":token}))?,
    )?;
    write_config(
        &root.join("mcp-stdio.mjs"),
        include_str!("../../../scripts/mcp-stdio.mjs").as_bytes(),
    )?;
    write_config(&root.join("ai-generate.mjs"),include_str!("../../../scripts/ai-generate.mjs").as_bytes())?;
    let workspace = workspace(s)?;
    for (name, text) in [("taoshu-import", SKILL), ("taoshu-workflow", WORKFLOW_SKILL)] {
        let skill = workspace.join(".qoder/skills").join(name);
        fs::create_dir_all(&skill)?;
        write_config(&skill.join("SKILL.md"), text.as_bytes())?;
    }
    let qoder_project = workspace.join(".mcp.json");
    let mut qoder_config: Value = if qoder_project.is_file() {
        serde_json::from_slice(&fs::read(&qoder_project)?)
            .map_err(|_| ApiError::bad("专用工作目录的 .mcp.json 不是有效 JSON，请先核对"))?
    } else { json!({}) };
    if !qoder_config.is_object() || (!qoder_config["mcpServers"].is_null() && !qoder_config["mcpServers"].is_object()) {
        return Err(ApiError::bad("Qoder MCP 配置必须为对象"));
    }
    qoder_config["mcpServers"]["taoshu"] = cfg["qoder"]["mcpServers"]["taoshu"].clone();
    write_config(&qoder_project, &serde_json::to_vec_pretty(&qoder_config)?)?;
    let skill = workspace.join(".opencode/skills/taoshu-import");
    fs::create_dir_all(&skill)?;
    write_config(&skill.join("SKILL.md"), SKILL.as_bytes())?;
    let workflow_skill = workspace.join(".opencode/skills/taoshu-workflow");
    fs::create_dir_all(&workflow_skill)?;
    write_config(&workflow_skill.join("SKILL.md"), WORKFLOW_SKILL.as_bytes())?;
    let project = workspace.join("opencode.json");
    let mut project_config: Value = if project.is_file() {
        serde_json::from_slice(&fs::read(&project)?)
            .map_err(|_| ApiError::bad("专用工作目录的 opencode.json 不是有效 JSON，请先核对"))?
    } else {
        json!({"$schema":"https://opencode.ai/config.json"})
    };
    if !project_config.is_object() {
        return Err(ApiError::bad("OpenCode 项目配置必须为对象"));
    }
    if (!project_config["mcp"].is_null() && !project_config["mcp"].is_object())
        || (!project_config["mcp"]["servers"].is_null()
            && !project_config["mcp"]["servers"].is_object())
    {
        return Err(ApiError::bad(
            "OpenCode MCP 配置格式无效，请先核对专用工作目录",
        ));
    }
    project_config["mcp"]["servers"]["taoshu"] =
        cfg["opencode"]["mcp"]["servers"]["taoshu"].clone();
    write_config(&project, &serde_json::to_vec_pretty(&project_config)?)?;
    write_config(
        &root.join("mcp-clients.json"),
        &serde_json::to_vec_pretty(&cfg["stdio"])?,
    )?;
    Ok(cfg)
}
async fn mcp_status(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let count: i64 = c.query_row("SELECT COUNT(*) FROM agent_imports", [], |r| r.get(0))?;
    let enabled = db::get_setting(&c, "agentMcpEnabled") == Some(json!(true));
    drop(c);
    Ok(Json(
        json!({"enabled":enabled,"endpoint":format!("{}/mcp",s.base_url()),"importCount":count,"tools":mcp::tools().iter().map(|t|t["name"].clone()).collect::<Vec<_>>(),"configs":configs(&s)?}),
    ))
}
async fn mcp_control(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let enabled = v["enabled"]
        .as_bool()
        .ok_or_else(|| ApiError::bad("请指定开启或关闭 MCP"))?;
    if enabled {
        prepare(&s)?;
    }
    db::set_setting(&lock(&s), "agentMcpEnabled", &json!(enabled))?;
    mcp_status(State(s)).await
}
async fn export(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    prepare(&s)?;
    mcp_status(State(s)).await
}
