use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::Context;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use rusqlite::Connection;

use crate::{db, models::ImportContext};

#[derive(Clone)]
pub struct AppState {
    pub xianyu: Arc<crate::xianyu::Gateway>,
    pub db: Arc<Mutex<Connection>>,
    pub data_dir: PathBuf,
    pub image_dir: PathBuf,
    pub token: String,
    pub bridge_secret: String,
    pub port: u16,
    pub import_ctx: Arc<Mutex<Option<ImportContext>>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf, port: u16) -> anyhow::Result<Self> {
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("创建数据目录失败: {}", data_dir.display()))?;
        let image_dir = data_dir.join("images");
        fs::create_dir_all(&image_dir)?;

        let conn = Connection::open(data_dir.join("shelf.db"))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        crate::commerce::schema::backup(&conn, &data_dir)?;
        db::init(&conn)?;
        conn.execute("UPDATE price_searches SET data=json_set(data,'$.state','waiting','$.error','应用已重启，请继续检索') WHERE json_extract(data,'$.state')='searching'", [])?;
        conn.execute(
            "UPDATE collection_jobs SET state='queued',stage='恢复任务' WHERE state='running'",
            [],
        )?;
        conn.execute("UPDATE xianyu_tasks SET status='unknown',error='应用在创建期间退出，请核查远端状态后继续' WHERE status='preparing'", [])?;

        let token_path = data_dir.join(".token");
        let token = match fs::read_to_string(&token_path) {
            Ok(value) if value.trim().len() >= 16 => value.trim().to_string(),
            _ => {
                let value = random_secret(24);
                fs::write(&token_path, &value)?;
                value
            }
        };

        Ok(Self {
            xianyu: Arc::new(crate::xianyu::Gateway::new()?),
            db: Arc::new(Mutex::new(conn)),
            data_dir,
            image_dir,
            token,
            bridge_secret: random_secret(24),
            port,
            import_ctx: Arc::new(Mutex::new(None)),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

pub fn lock(state: &AppState) -> MutexGuard<'_, Connection> {
    state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn random_secret(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}
