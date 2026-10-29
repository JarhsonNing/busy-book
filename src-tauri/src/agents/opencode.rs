use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    time::Instant,
};
pub const VERSION: &str = "2.2.0";
pub(super) fn managed_path(s: &AppState) -> PathBuf {
    s.data_dir.join("runtime/opencode").join(format!(
        "opencode-{VERSION}{}",
        std::env::consts::EXE_SUFFIX
    ))
}
fn hidden(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
}
pub(super) fn bounded(mut cmd: Command, seconds: u64) -> Result<String, String> {
    hidden(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("程序无法启动：{e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err(format!(
                        "程序返回失败状态 {status}：{}",
                        String::from_utf8_lossy(&output.stderr)
                            .chars()
                            .take(500)
                            .collect::<String>()
                    ));
                }
                return Ok(String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .chars()
                    .take(2000)
                    .collect());
            }
            None => {
                if start.elapsed() > Duration::from_secs(seconds) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("程序执行超时，请检查安装与运行环境".into());
                }
                std::thread::sleep(Duration::from_millis(80));
            }
        }
    }
}
pub(super) fn version(path: &Path, dir: &Path) -> Result<String, String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version").current_dir(dir);
    let result = bounded(cmd, 8)?;
    if !result.to_lowercase().contains("opencode") {
        return Err("所选程序没有返回 OpenCode 版本".into());
    }
    if !result
        .split_whitespace()
        .any(|word| word.trim_start_matches('v').starts_with("2."))
    {
        return Err("需要 OpenCode v2，可安装应用专用版".into());
    }
    Ok(result)
}
pub(super) fn discover() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in std::env::var_os("PATH")
        .into_iter()
        .flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
    {
        let known = dir.join("node_modules/@opencode/cli/bin/opencode.exe");
        if cfg!(windows) && known.is_file() {
            if !found.contains(&known) {
                found.push(known);
            }
            continue;
        }
        for name in if cfg!(windows) {
            vec!["opencode.exe", "opencode.cmd"]
        } else {
            vec!["opencode"]
        } {
            let p = dir.join(name);
            if p.is_file() && !found.contains(&p) {
                found.push(p);
            }
        }
    }
    found
}
pub(super) fn selection(s: &AppState) -> Value {
    db::get_setting(&lock(s), "agentOpenCode").unwrap_or(json!({"mode":"local"}))
}
pub(super) fn selected(s: &AppState) -> Result<PathBuf, ApiError> {
    let cfg = selection(s);
    if cfg["mode"] == "managed" {
        let p = managed_path(s);
        if p.is_file() {
            Ok(p)
        } else {
            Err(ApiError::bad("请先安装应用专用版"))
        }
    } else if let Some(p) = cfg["path"].as_str().filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        if p.is_file() {
            Ok(p)
        } else {
            Err(ApiError::bad("本机 OpenCode 已移动，请重新选择"))
        }
    } else {
        discover()
            .into_iter()
            .find(|p| workspace(s).is_ok_and(|dir| version(p, &dir).is_ok()))
            .ok_or_else(|| ApiError::bad("未检测到本机 OpenCode，请安装或选择程序"))
    }
}
pub(super) async fn status(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let ss = s.clone();
    let dir = workspace(&s)?;
    let local = tokio::task::spawn_blocking(move || {
        let mut paths = discover();
        if let Some(p) = selection(&ss)["path"].as_str() {
            let p = PathBuf::from(p);
            if p.is_file() && !paths.contains(&p) {
                paths.insert(0, p);
            }
        }
        paths
            .into_iter()
            .take(8)
            .map(|path| match version(&path, &dir) {
                Ok(v) => json!({"path":path,"version":v,"available":true}),
                Err(e) => json!({"path":path,"error":e,"available":false}),
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| ApiError::bad(e.to_string()))?;
    let p = managed_path(&s);
    let installed = p.is_file();
    let cfg = selection(&s);
    let selected = selected(&s).ok();
    Ok(Json(
        json!({"mode":cfg["mode"],"local":local,"selectedPath":selected,"workspace":workspace(&s)?,"managed":{"path":p,"installed":installed,"version":VERSION},"install":db::get_setting(&lock(&s),"agentOpenCodeInstall").unwrap_or(json!({"state":"idle"}))}),
    ))
}
pub(super) async fn select(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let mut service = s.opencode_web.lock().await;
    if web::snapshot(&mut service)["running"] == true {
        return Err(ApiError::bad("请先停止 OpenCode 再切换程序"));
    }
    let mode = v["mode"].as_str().unwrap_or("");
    if !["local", "managed"].contains(&mode) {
        return Err(ApiError::bad("请选择本机或应用专用版"));
    }
    if mode == "local" {
        if let Some(path) = v["path"].as_str().filter(|p| !p.is_empty()) {
            let p = PathBuf::from(path);
            if !p.is_absolute() || !p.is_file() {
                return Err(ApiError::bad("请选择已安装的 OpenCode 程序"));
            }
            let check = p.clone();
            let dir = workspace(&s)?;
            tokio::task::spawn_blocking(move || version(&check, &dir))
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
    db::set_setting(
        &lock(&s),
        "agentOpenCode",
        &json!({"mode":mode,"path":path}),
    )?;
    drop(service);
    status(State(s)).await
}
pub(super) async fn browse(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    if !cfg!(windows) {
        return Err(ApiError::bad("此平台请从检测列表选择 OpenCode"));
    }
    let dir = workspace(&s)?;
    let chosen=tokio::task::spawn_blocking(move ||{
        let mut cmd=Command::new("powershell.exe");cmd.args(["-NoProfile","-NonInteractive","-STA","-Command","Add-Type -AssemblyName System.Windows.Forms; $picker = New-Object System.Windows.Forms.OpenFileDialog; $picker.Title = '选择 OpenCode 程序'; $picker.Filter = 'OpenCode|opencode.exe;opencode.cmd'; if ($picker.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::OutputEncoding = [Text.Encoding]::UTF8; [Console]::Write($picker.FileName) }"]);
        cmd.current_dir(dir);
        bounded(cmd,300)
    }).await.map_err(|e|ApiError::bad(e.to_string()))?.map_err(ApiError::bad)?;
    if chosen.is_empty() {
        return Ok(Json(json!({"canceled":true})));
    }
    select(
        State(s),
        Json(json!({"mode":"local","path":chosen.trim_start_matches('\u{feff}')})),
    )
    .await
}
fn progress(s: &AppState, stage: &str, downloaded: u64, total: u64) {
    let _ = db::set_setting(
        &lock(s),
        "agentOpenCodeInstall",
        &json!({"state":"running","stage":stage,"downloaded":downloaded,"total":total,"version":VERSION}),
    );
}
pub(super) fn package_asset(v: &Value, name: &str) -> Result<(String, String), String> {
    if v["name"] != name || v["version"] != VERSION {
        return Err("官方包版本不匹配".into());
    }
    let raw = v["dist"]["tarball"].as_str().ok_or("官方地址缺失")?;
    let expected = format!(
        "https://registry.npmjs.org/{name}/-/{}-{VERSION}.tgz",
        name.rsplit('/').next().unwrap()
    );
    if raw != expected {
        return Err("下载地址不属于官方发行包".into());
    }
    let digest = v["dist"]["integrity"]
        .as_str()
        .and_then(|v| {
            v.strip_prefix("sha512:")
                .or_else(|| v.strip_prefix("sha512-"))
        })
        .ok_or("官方包缺少 SHA-512 校验值")?;
    use base64::{engine::general_purpose::STANDARD, Engine};
    if STANDARD.decode(digest).map_err(|_| "无效校验值")?.len() != 64 {
        return Err("无效校验值".into());
    }
    Ok((raw.into(), digest.into()))
}
pub(super) async fn download(
    client: &reqwest::Client,
    s: &AppState,
    url: &str,
    archive: &Path,
) -> Result<(), String> {
    let offset = fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let mut response = request
        .send()
        .await
        .map_err(|e| format!("下载连接失败：{e:?}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if resumed
        && !response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with(&format!("bytes {offset}-")))
    {
        return Err("续传范围不匹配".into());
    }
    let offset = if resumed { offset } else { 0 };
    let total = offset + response.content_length().unwrap_or(0);
    if total > 200 * 1024 * 1024 {
        return Err("官方安装包超过大小限制".into());
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(!resumed)
        .append(resumed)
        .write(true)
        .open(archive)
        .map_err(|e| e.to_string())?;
    let mut downloaded = offset;
    let mut reported = offset;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("下载中断：{e:?}"))?
    {
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
    Ok(())
}
async fn install_inner(s: &AppState) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("自动安装目前支持 Windows，请使用本机已安装版本".into());
    }
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else if std::env::consts::ARCH == "x86_64" {
        "x64-baseline"
    } else {
        return Err("不支持的 CPU 架构".into());
    };
    let destination = managed_path(s);
    let dir = workspace(s).map_err(|e| format!("{e:?}"))?;
    if destination.is_file() {
        let p = destination.clone();
        tokio::task::spawn_blocking(move || version(&p, &dir))
            .await
            .map_err(|e| e.to_string())??;
        return Ok(());
    }
    progress(s, "获取官方版本", 0, 0);
    let client = reqwest::Client::builder()
        .user_agent("taoshu-studio")
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let package = format!("@opencode/cli-windows-{arch}");
    let release: Value = client
        .get(format!(
            "https://registry.npmjs.org/{}/{VERSION}",
            package.replace('/', "%2f")
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let (url, digest) = package_asset(&release, &package)?;
    let root = destination.parent().unwrap();
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let suffix = rand::random::<u64>();
    let archive = root.join(format!("download-{suffix}.tgz"));
    let candidate = root.join(format!("candidate-{suffix}.exe"));
    let result = async {
        if download(&client, s, &url, &archive).await.is_err() {
            progress(s, "重新下载安装包", 0, 0);
            download(&client, s, &url, &archive).await?;
        }
        let script = root.join("install-opencode.mjs");
        fs::write(
            &script,
            include_str!("../../../scripts/install-opencode.mjs"),
        )
        .map_err(|e| e.to_string())?;
        let node = super::node_path().ok_or("未找到应用 Node 运行环境")?;
        let package = archive.clone();
        let target = candidate.clone();
        let check_dir = dir.clone();
        tokio::task::spawn_blocking(move || {
            let mut cmd = Command::new(node);
            cmd.arg(script).arg(package).arg(target).arg(digest).current_dir(check_dir);
            bounded(cmd, 60)
        })
        .await
        .map_err(|e| e.to_string())??;
        let candidate_copy = candidate.clone();
        let detected = tokio::task::spawn_blocking(move || version(&candidate_copy, &dir))
            .await
            .map_err(|e| e.to_string())??;
        if !detected.contains(VERSION) {
            return Err("程序版本与官方安装包不一致".into());
        }
        fs::rename(&candidate, &destination).map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;
    let _ = fs::remove_file(archive);
    let _ = fs::remove_file(candidate);
    result
}
pub(super) async fn install(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    {
        let c = lock(&s);
        if db::get_setting(&c, "agentOpenCodeInstall").is_some_and(|v| v["state"] == "running") {
            return Err(ApiError::bad("正在安装，请等待当前任务完成"));
        }
        db::set_setting(
            &c,
            "agentOpenCodeInstall",
            &json!({"state":"running","stage":"准备安装","version":VERSION}),
        )?;
    }
    let ss = s.clone();
    tokio::spawn(async move {
        let result = install_inner(&ss).await;
        let data = match result {
            Ok(()) => json!({"state":"completed","stage":"已安装","version":VERSION}),
            Err(error) => json!({"state":"failed","error":error,"version":VERSION}),
        };
        let _ = db::set_setting(&lock(&ss), "agentOpenCodeInstall", &data);
    });
    Ok(Json(json!({"state":"running"})))
}
