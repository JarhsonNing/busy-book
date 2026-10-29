//! Own a private headless server and its web gateway, never the user's shared service.
use super::*;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Instant,
};

#[derive(Default)]
pub struct Runtime {
    service: Option<Service>,
}
struct Service {
    child: Child,
    url: String,
    mode: String,
    #[cfg(windows)]
    _job: ProcessJob,
}
#[cfg(windows)]
pub(super) struct ProcessJob(windows_sys::Win32::Foundation::HANDLE);
// A uniquely owned Windows job handle may be closed from any thread.
#[cfg(windows)]
unsafe impl Send for ProcessJob {}
#[cfg(windows)]
impl ProcessJob {
    pub(super) fn attach(child: &Child) -> std::io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(handle);
                return Err(error);
            }
            Ok(Self(handle))
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        // Closing stdin asks the gateway to stop its own OpenCode child. The job also covers crashes.
        drop(self.child.stdin.take());
        let start = Instant::now();
        while self.child.try_wait().ok().flatten().is_none()
            && start.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(30));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub(super) fn snapshot(runtime: &mut Runtime) -> Value {
    if runtime
        .service
        .as_mut()
        .is_some_and(|v| v.child.try_wait().ok().flatten().is_some())
    {
        runtime.service.take();
    }
    runtime
        .service
        .as_ref()
        .map(|v| json!({"running":true,"url":v.url,"mode":v.mode}))
        .unwrap_or(json!({"running":false}))
}
pub(super) async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(snapshot(&mut *s.opencode_web.lock().await))
}
pub(super) async fn stop(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut runtime = s.opencode_web.lock().await;
    let service = runtime.service.take();
    tokio::task::spawn_blocking(move || drop(service))
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(Json(json!({"running":false})))
}
fn spawn(s: &AppState, path: PathBuf, cfg: Value, mode: String) -> Result<Service, ApiError> {
    let dir = workspace(s)?;
    opencode::version(&path, &dir).map_err(ApiError::bad)?;
    let script = support(s).join("opencode-web.mjs");
    write_config(
        &script,
        include_str!("../../../scripts/opencode-web.mjs").as_bytes(),
    )?;
    let profile = if mode == "managed" {
        let p = support(s).join("opencode-profile");
        for name in ["config", "data", "cache", "state"] {
            fs::create_dir_all(p.join(name))?;
        }
        p.to_string_lossy().to_string()
    } else {
        String::new()
    };
    let mut command =
        Command::new(node_path().ok_or_else(|| ApiError::bad("未找到应用 Node 运行环境"))?);
    command
        .arg(script)
        .arg(path)
        .arg(cfg["workspace"].as_str().unwrap())
        .arg(profile)
        .arg(std::process::id().to_string())
        .current_dir(&dir)
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
    let job = match ProcessJob::attach(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ApiError::bad(format!("无法管理 OpenCode 服务进程：{e}")));
        }
    };
    let output = child.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(output)
            .take(16384)
            .read_line(&mut line)
            .map(|_| line);
        let _ = send.send(result);
    });
    let started = receive
        .recv_timeout(Duration::from_secs(30))
        .ok()
        .and_then(|r| r.ok())
        .and_then(|line| serde_json::from_str::<Value>(&line).ok());
    let url = started
        .as_ref()
        .and_then(|v| v["url"].as_str())
        .filter(|u| valid_web_url(u));
    if let Some(url) = url {
        Ok(Service {
            child,
            url: url.into(),
            mode,
            #[cfg(windows)]
            _job: job,
        })
    } else {
        let _ = child.kill();
        let _ = child.wait();
        Err(ApiError::bad(
            "OpenCode 网页服务启动失败，请重新检测或安装应用专用版",
        ))
    }
}
fn valid_web_url(raw: &str) -> bool {
    reqwest::Url::parse(raw).ok().is_some_and(|u| {
        u.scheme() == "http"
            && u.port().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.path() == "/"
            && u.query().is_none()
            && u.fragment().is_none()
            && u.host_str().is_some_and(|host| {
                host.strip_suffix(".localhost").is_some_and(|key| {
                    key.len() == 48 && key.bytes().all(|b| b.is_ascii_hexdigit())
                })
            })
    })
}
pub(super) async fn start(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    if db::get_setting(&lock(&s), "agentMcpEnabled") != Some(json!(true)) {
        return Err(ApiError::bad("请先开启 MCP 导入"));
    }
    let mut runtime = s.opencode_web.lock().await;
    if snapshot(&mut runtime)["running"] == true {
        return Ok(Json(snapshot(&mut runtime)));
    }
    let path = opencode::selected(&s)?;
    let mode = opencode::selection(&s)["mode"]
        .as_str()
        .unwrap_or("local")
        .to_owned();
    let cfg = prepare(&s)?;
    let worker = s.clone();
    runtime.service = Some(
        tokio::task::spawn_blocking(move || spawn(&worker, path, cfg, mode))
            .await
            .map_err(|e| ApiError::bad(e.to_string()))??,
    );
    Ok(Json(snapshot(&mut runtime)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn web_url_is_only_the_private_loopback_origin() {
        assert!(super::valid_web_url(&format!(
            "http://{}.localhost:12345",
            "a".repeat(48)
        )));
        for value in [
            "http://127.0.0.1:12345",
            "https://evil.example",
            "http://key.localhost:12345",
            "http://user:pass@localhost:12345",
        ] {
            assert!(!super::valid_web_url(value));
        }
    }
}
