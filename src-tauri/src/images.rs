use std::{io::Cursor, path::Path, time::Duration};

use anyhow::{bail, Context};
use chrono::Utc;
use rand::Rng;

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub struct SavedImage {
    pub file: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub content_type: String,
}

pub async fn download(url: &str, referer: &str, dir: &Path) -> anyhow::Result<SavedImage> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        bail!("只支持 http/https 图片地址");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .user_agent(UA)
        .build()?;

    let mut request = client.get(url).header(
        "accept",
        "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
    );
    if !referer.is_empty() {
        request = request.header("referer", referer);
    }

    let response = request
        .send()
        .await
        .with_context(|| format!("下载失败: {url}"))?;
    if !response.status().is_success() {
        bail!("图片下载失败 HTTP {}", response.status());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if let Some(length) = response.content_length() {
        if length > 20 * 1024 * 1024 {
            bail!("图片超过 20MB");
        }
    }
    let bytes = response.bytes().await?;
    if bytes.len() > 20 * 1024 * 1024 {
        bail!("图片超过 20MB");
    }
    if bytes.len() < 200 {
        bail!("图片内容为空");
    }

    // 有些图床返回 application/octet-stream，按文件头兜底判断
    let content_type = if content_type.starts_with("image/") {
        content_type
    } else {
        sniff_type(&bytes)
            .ok_or_else(|| anyhow::anyhow!("返回内容不是图片（{content_type}）"))?
            .to_string()
    };

    save_bytes(&bytes, &content_type, dir, "o")
}

fn sniff_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        return Some("image/png");
    }
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

pub fn save_bytes(
    bytes: &[u8],
    content_type: &str,
    dir: &Path,
    prefix: &str,
) -> anyhow::Result<SavedImage> {
    if bytes.len() > 40 * 1024 * 1024 {
        bail!("图片超过 40MB");
    }
    let content_type = normalize_type(content_type);
    if !content_type.starts_with("image/") {
        bail!("不支持的内容类型: {content_type}");
    }
    let extension = extension_for(&content_type);
    let file = format!(
        "{}-{}-{}.{}",
        prefix,
        Utc::now().timestamp_millis(),
        random_suffix(),
        extension
    );
    std::fs::write(dir.join(&file), bytes)?;
    let (width, height) = dimensions(bytes);
    Ok(SavedImage {
        file,
        width,
        height,
        content_type,
    })
}

pub fn remove_file(dir: &Path, file: &str) {
    if file.is_empty() {
        return;
    }
    let name = Path::new(file)
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let _ = std::fs::remove_file(dir.join(name));
}

pub fn mime_for(file: &str) -> &'static str {
    match file
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "image/jpeg",
    }
}

fn normalize_type(content_type: &str) -> String {
    let value = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if value.is_empty() {
        "image/jpeg".into()
    } else if value == "image/jpg" {
        "image/jpeg".into()
    } else {
        value
    }
}

fn extension_for(content_type: &str) -> &'static str {
    match content_type {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        _ => "jpg",
    }
}

fn dimensions(bytes: &[u8]) -> (Option<i64>, Option<i64>) {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .map(|(width, height)| (Some(width as i64), Some(height as i64)))
        .unwrap_or((None, None))
}

fn random_suffix() -> String {
    let value: u32 = rand::thread_rng().gen();
    format!("{value:08x}")
}
