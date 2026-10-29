//! Qoder's native interactive CLI, always launched in the app-owned project.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
};

const RELEASES: &str = "https://qoder-ide.oss-accelerate.aliyuncs.com/qodercli";
#[derive(Default)]
pub struct Runtime {
    terminal: Option<Terminal>,
}
struct Terminal {
    child: Child,
    #[cfg(windows)]
    _job: web::ProcessJob,
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn snapshot(runtime: &mut Runtime) -> Value {
    if runtime
        .terminal
        .as_mut()
        .is_some_and(|t| t.child.try_wait().ok().flatten().is_some())
    {
        runtime.terminal.take();
    }
    json!({"running":runtime.terminal.is_some()})
}
fn managed_path(s: &AppState) -> PathBuf {
    s.data_dir.join("runtime/qoder/qoder.exe")
}
fn selection(s: &AppState) -> Value {
    db::get_setting(&lock(s), "agentQoder").unwrap_or(json!({"mode":"local"}))
}
fn discover() -> Vec<PathBuf> {
    let mut dirs = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = PathBuf::from(home);
        dirs.extend([home.join(".local/bin"), home.join(".qoder/bin")]);
    }
    let mut paths = Vec::new();
    for dir in dirs {
        for name in if cfg!(windows) {
            vec!["qoder.exe", "qodercli.exe", "qoder.cmd", "qodercli.cmd"]
        } else {
            vec!["qoder", "qodercli"]
        } {
            let path = dir.join(name);
            if path.is_file() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}
fn version(path: &Path, dir: &Path) -> Result<String, String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version").current_dir(dir);
    let result = opencode::bounded(cmd, 10)?;
    // Native releases print either a bare semver or a branded version string.
    if !regex::Regex::new(
        r"(?i)^(?:(?:qoder(?:cli)?|@qoder-ai/qodercli)\s+)?v?\d+\.\d+\.\d+(?:[^\r\n]*)?$",
    )
    .unwrap()
    .is_match(&result)
    {
        return Err("所选程序没有返回有效的 Qoder 版本".into());
    }
    Ok(result)
}
pub(super) fn selected(s: &AppState) -> Result<PathBuf, ApiError> {
    let cfg = selection(s);
    let dir = workspace(s)?;
    let path = if cfg["mode"] == "managed" {
        managed_path(s)
    } else if let Some(p) = cfg["path"].as_str().filter(|p| !p.is_empty()) {
        PathBuf::from(p)
    } else {
        discover()
            .into_iter()
            .find(|p| version(p, &dir).is_ok())
            .ok_or_else(|| ApiError::bad("未检测到 Qoder CLI，请安装专用版或选择本机程序"))?
    };
    if !path.is_absolute() || !path.is_file() {
        return Err(ApiError::bad("Qoder 程序不存在，请重新选择或安装"));
    }
    Ok(path)
}
pub(super) async fn status(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let ss = s.clone();
    let local = tokio::task::spawn_blocking(move || {
        let dir = workspace(&ss)?;
        let mut paths = discover();
        if let Some(p) = selection(&ss)["path"].as_str() {
            let p = PathBuf::from(p);
            if !paths.contains(&p) {
                paths.insert(0, p);
            }
        }
        Ok::<_, ApiError>(
            paths
                .into_iter()
                .take(8)
                .map(|path| match version(&path, &dir) {
                    Ok(v) => json!({"path":path,"version":v,"available":true}),
                    Err(e) => json!({"path":path,"error":e,"available":false}),
                })
                .collect::<Vec<_>>(),
        )
    })
    .await
    .map_err(|e| ApiError::bad(e.to_string()))??;
    let cfg = selection(&s);
    let path = managed_path(&s);
    let selected = if cfg["mode"] == "managed" {
        path.is_file().then(|| path.clone())
    } else {
        local
            .iter()
            .find(|v| {
                v["available"] == true
                    && (cfg["path"].as_str().filter(|p| !p.is_empty()).is_none()
                        || v["path"] == cfg["path"])
            })
            .and_then(|v| v["path"].as_str())
            .map(PathBuf::from)
    };
    Ok(Json(
        json!({"mode":cfg["mode"],"local":local,"selectedPath":selected,"workspace":workspace(&s)?,"managed":{"path":path,"installed":path.is_file()},"install":db::get_setting(&lock(&s),"agentQoderInstall").unwrap_or(json!({"state":"idle"}))}),
    ))
}
pub(super) async fn select(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let mut runtime = s.qoder_cli.lock().await;
    if snapshot(&mut runtime)["running"] == true {
        return Err(ApiError::bad("请先关闭 Qoder 终端再切换程序"));
    }
    let mode = v["mode"].as_str().unwrap_or("");
    if !["local", "managed"].contains(&mode) {
        return Err(ApiError::bad("请选择本机或应用专用版"));
    }
    if mode == "local" {
        if let Some(path) = v["path"].as_str().filter(|p| !p.is_empty()) {
            let p = PathBuf::from(path);
            if !p.is_absolute() || !p.is_file() {
                return Err(ApiError::bad("请选择已安装的 Qoder 程序"));
            }
            let dir = workspace(&s)?;
            tokio::task::spawn_blocking(move || version(&p, &dir))
                .await
                .map_err(|e| ApiError::bad(e.to_string()))?
                .map_err(ApiError::bad)?;
        }
    }
    let path = if mode == "local" {
        v["path"].clone()
    } else {
        selection(&s)["path"].clone()
    };
    db::set_setting(&lock(&s), "agentQoder", &json!({"mode":mode,"path":path}))?;
    drop(runtime);
    status(State(s)).await
}
pub(super) async fn browse(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    if !cfg!(windows) {
        return Err(ApiError::bad("此平台请从检测列表选择 Qoder"));
    }
    let dir = workspace(&s)?;
    let path=tokio::task::spawn_blocking(move || {
        let mut cmd=Command::new("powershell.exe");
        cmd.current_dir(dir).args(["-NoProfile","-NonInteractive","-STA","-Command","Add-Type -AssemblyName System.Windows.Forms; $picker = New-Object System.Windows.Forms.OpenFileDialog; $picker.Title = '选择 Qoder CLI'; $picker.Filter = 'Qoder|qoder.exe;qodercli.exe;qoder.cmd;qodercli.cmd'; if ($picker.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::OutputEncoding = [Text.Encoding]::UTF8; [Console]::Write($picker.FileName) }"]);
        opencode::bounded(cmd,300)
    }).await.map_err(|e|ApiError::bad(e.to_string()))?.map_err(ApiError::bad)?;
    if path.is_empty() {
        return Ok(Json(json!({"canceled":true})));
    }
    select(
        State(s),
        Json(json!({"mode":"local","path":path.trim_start_matches('\u{feff}')})),
    )
    .await
}
pub(super) async fn cli_status(State(s): State<AppState>) -> Json<Value> {
    Json(snapshot(&mut *s.qoder_cli.lock().await))
}
pub(super) async fn stop(State(s): State<AppState>) -> Json<Value> {
    s.qoder_cli.lock().await.terminal.take();
    Json(json!({"running":false}))
}
pub(super) async fn launch(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    launch_terminal(s, false).await
}
pub(super) async fn login(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    launch_terminal(s, true).await
}
pub(super) async fn account(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let dir = workspace(&s)?;
        let mut cmd = Command::new(selected(&s)?);
        cmd.args(["--cwd"])
            .arg(&dir)
            .args(["status", "-o", "json"])
            .current_dir(dir);
        let raw = opencode::bounded(cmd, 20).map_err(|_| {
            ApiError::bad("无法读取 Qoder 登录状态，请重新检测程序或在终端检查登录")
        })?;
        let status: Value = serde_json::from_str(&raw)
            .map_err(|_| ApiError::bad("Qoder 登录状态格式无法识别，请更新 CLI 后重试"))?;
        // The CLI status can include account identifiers. Return only the login flag.
        Ok(Json(json!({"loggedIn":status["logged_in"].as_bool()})))
    })
    .await
    .map_err(|e| ApiError::bad(e.to_string()))?
}
pub(super) fn parse_models(raw: &str) -> Result<Vec<String>, String> {
    // Native --list-models prints MODEL, then system names or "name (custom-key)".
    let mut items = Vec::new();
    for line in raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "MODEL")
    {
        let candidate = if line.ends_with(')') {
            line.rsplit_once(" (")
                .map(|(_, id)| id.trim_end_matches(')'))
                .unwrap_or(line)
        } else {
            line
        };
        if candidate.ends_with('…') || candidate.ends_with("...") {
            continue;
        }
        if candidate.len() <= 200
            && !candidate.starts_with('-')
            && candidate
                .chars()
                .all(|c| c.is_alphanumeric() || " /._-:#".contains(c))
            && !items.iter().any(|v| v == candidate)
        {
            items.push(candidate.to_string());
        }
    }
    if !raw.lines().any(|line| line.trim() == "MODEL") {
        return Err("Qoder 模型列表格式无法识别，请更新 CLI 后重试".into());
    }
    Ok(items)
}
async fn launch_terminal(s: AppState, login: bool) -> Result<Json<Value>, ApiError> {
    if !cfg!(windows) {
        return Err(ApiError::bad("Qoder 终端启动目前支持 Windows"));
    }
    if !login && db::get_setting(&lock(&s), "agentMcpEnabled") != Some(json!(true)) {
        return Err(ApiError::bad("请先开启 MCP 导入"));
    }
    let mut runtime = s.qoder_cli.lock().await;
    if snapshot(&mut runtime)["running"] == true {
        if login {
            return Err(ApiError::bad("请先关闭现有 Qoder 终端，再打开登录窗口"));
        }
        return Ok(Json(snapshot(&mut runtime)));
    }
    let worker = s.clone();
    runtime.terminal = Some(
        tokio::task::spawn_blocking(move || {
            let executable = selected(&worker)?;
            let dir = workspace(&worker)?;
            version(&executable, &dir).map_err(ApiError::bad)?;
            if !login {
                prepare(&worker)?;
            }
            let ai = ai::config(&lock(&worker));
            let terminal_model = ai["models"]["qoder"].as_str().unwrap_or("").to_string();
            let options = support(&worker).join("qoder-terminal.json");
            write_config(
                &options,
                &serde_json::to_vec(&json!({"login":login,"model":terminal_model}))?,
            )?;
            let script = support(&worker).join("qoder-terminal.ps1");
            write_config(
                &script,
                &[
                    b"\xef\xbb\xbf".as_slice(),
                    include_bytes!("../../../scripts/qoder-terminal.ps1").as_slice(),
                ]
                .concat(),
            )?;
            let mut command = Command::new("powershell.exe");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script)
                .arg("-HostTerminal")
                .arg("-Program")
                .arg(executable)
                .arg("-Workspace")
                .arg(&dir)
                .arg("-Options")
                .arg(&options)
                .current_dir(dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command.spawn()?;
            #[cfg(windows)]
            let job = match web::ProcessJob::attach(&child) {
                Ok(job) => job,
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ApiError::bad(format!("无法管理 Qoder 终端：{e}")));
                }
            };
            if let Err(e) = child.stdin.take().unwrap().write_all(b"start\n") {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.into());
            }
            let stdout = child.stdout.take().unwrap();
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let mut line = String::new();
                let _ = BufReader::new(stdout).read_line(&mut line);
                let _ = send.send(line);
            });
            if receive
                .recv_timeout(Duration::from_secs(15))
                .ok()
                .as_deref()
                != Some("ready\r\n")
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ApiError::bad("Qoder 终端启动失败，请重新检测程序后重试"));
            }
            Ok::<_, ApiError>(Terminal {
                child,
                #[cfg(windows)]
                _job: job,
            })
        })
        .await
        .map_err(|e| ApiError::bad(e.to_string()))??,
    );
    Ok(Json(snapshot(&mut runtime)))
}

fn progress(s: &AppState, stage: &str, downloaded: u64, total: u64) {
    let _ = db::set_setting(
        &lock(s),
        "agentQoderInstall",
        &json!({"state":"running","stage":stage,"downloaded":downloaded,"total":total}),
    );
}
fn package_asset(v: &Value) -> Result<(String, String, String), String> {
    let version = v["latest"]
        .as_str()
        .filter(|s| regex::Regex::new(r"^\d+\.\d+\.\d+$").unwrap().is_match(s))
        .ok_or("官方版本号无效")?;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return Err("不支持的 CPU 架构".into()),
    };
    let file = v["files"]
        .as_array()
        .and_then(|files| {
            files
                .iter()
                .find(|f| f["os"] == "windows" && f["arch"] == arch && f["variant"] == "standard")
        })
        .ok_or("没有兼容的 Qoder Windows 原生版本")?;
    let name = if arch == "amd64" { "x64" } else { "arm64" };
    let expected = format!("{RELEASES}/releases/{version}/qodercli-windows-{name}.zip");
    if file["url"] != expected {
        return Err("下载地址不属于官方发行包".into());
    }
    let digest = file["sha256"]
        .as_str()
        .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("官方包缺少 SHA-256 校验值")?;
    Ok((version.into(), expected, digest.to_ascii_lowercase()))
}
async fn install_inner(s: &AppState) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("自动安装目前支持 Windows，请使用本机已安装版本".into());
    }
    let dir = workspace(s).map_err(|e| format!("{e:?}"))?;
    let destination = managed_path(s);
    if destination.is_file() {
        return version(&destination, &dir);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    progress(s, "获取官方版本", 0, 0);
    let release: Value = client
        .get(format!("{RELEASES}/channels/manifest.json"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let (expected, url, digest) = package_asset(&release)?;
    let root = destination.parent().unwrap();
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let suffix = rand::random::<u64>();
    let archive = root.join(format!("download-{suffix}.zip"));
    let candidate = root.join(format!("candidate-{suffix}.exe"));
    let result = async {
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let total = response.content_length().unwrap_or(0);
        let mut downloaded = 0;
        let mut reported = 0;
        if total > 200 * 1024 * 1024 {
            return Err("官方安装包超过大小限制".into());
        }
        let mut file = fs::File::create(&archive).map_err(|e| e.to_string())?;
        progress(s, "下载程序", 0, total);
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            downloaded += chunk.len() as u64;
            if downloaded > 200 * 1024 * 1024 {
                return Err("下载超过大小限制".into());
            }
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            if downloaded - reported >= 1024 * 1024 {
                progress(s, "下载程序", downloaded, total);
                reported = downloaded;
            }
        }
        drop(file);
        progress(s, "校验并解压", downloaded, total);
        let script = root.join("install-qoder.ps1");
        fs::write(
            &script,
            [
                b"\xef\xbb\xbf".as_slice(),
                include_bytes!("../../../scripts/install-qoder.ps1").as_slice(),
            ]
            .concat(),
        )
        .map_err(|e| e.to_string())?;
        let target = candidate.clone();
        let package = archive.clone();
        let check_dir = dir.clone();
        tokio::task::spawn_blocking(move || {
            let mut cmd = Command::new("powershell.exe");
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .arg("-Archive")
            .arg(package)
            .arg("-Destination")
            .arg(target)
            .arg("-Sha256")
            .arg(digest)
            .current_dir(check_dir);
            opencode::bounded(cmd, 60)
        })
        .await
        .map_err(|e| e.to_string())??;
        let check = candidate.clone();
        let check_dir = dir.clone();
        let detected = tokio::task::spawn_blocking(move || version(&check, &check_dir))
            .await
            .map_err(|e| e.to_string())??;
        if !detected.contains(&expected) {
            return Err("程序版本与官方安装包不一致".into());
        }
        fs::rename(&candidate, &destination).map_err(|e| e.to_string())?;
        Ok(expected)
    }
    .await;
    let _ = fs::remove_file(archive);
    let _ = fs::remove_file(candidate);
    result
}
pub(super) async fn install(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    {
        let c = lock(&s);
        if db::get_setting(&c, "agentQoderInstall").is_some_and(|v| v["state"] == "running") {
            return Err(ApiError::bad("正在安装，请等待当前任务完成"));
        }
        db::set_setting(
            &c,
            "agentQoderInstall",
            &json!({"state":"running","stage":"准备安装"}),
        )?;
    }
    tokio::spawn(async move {
        let data = match install_inner(&s).await {
            Ok(version) => json!({"state":"completed","stage":"已安装","version":version}),
            Err(error) => json!({"state":"failed","error":error}),
        };
        let _ = db::set_setting(&lock(&s), "agentQoderInstall", &data);
    });
    Ok(Json(json!({"state":"running"})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_model_list_uses_complete_custom_keys_and_skips_truncated_ones() {
        assert_eq!(
            parse_models("MODEL\nAuto\nPerformance\n个人模型 (byok-123)\n长模型 (abc…)\nAuto\n")
                .unwrap(),
            vec!["Auto", "Performance", "byok-123"]
        );
        assert!(parse_models("Not logged in. Run login").is_err());
        assert_eq!(parse_models("MODEL\n").unwrap(), Vec::<String>::new());
    }
    #[test]
    fn manifest_requires_official_url_and_digest() {
        let arch = if std::env::consts::ARCH == "aarch64" {
            "arm64"
        } else {
            "amd64"
        };
        let name = if arch == "arm64" { "arm64" } else { "x64" };
        let mut v = json!({"latest":"1.2.3","files":[{"os":"windows","arch":arch,"variant":"standard","url":format!("{RELEASES}/releases/1.2.3/qodercli-windows-{name}.zip"),"sha256":"a".repeat(64)}]});
        assert!(package_asset(&v).is_ok());
        v["files"][0]["sha256"] = json!("bad");
        assert!(package_asset(&v).is_err());
        v["files"][0]["sha256"] = json!("a".repeat(64));
        v["files"][0]["url"] = json!("https://evil.example/qoder.zip");
        assert!(package_asset(&v).is_err());
    }
}
