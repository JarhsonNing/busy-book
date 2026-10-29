//! Douban metadata only: fixed hosts, no cookies, no verification bypass, persistent cache.
use super::*;
use regex::Regex;
use rusqlite::{params, OptionalExtension};
use std::{io::Read, sync::Mutex, time::Instant};
type Result<T> = std::result::Result<T, String>;
const ORIGIN: &str = "https://book.douban.com";
#[derive(Default)]
struct Rate {
    last: Option<Instant>,
    blocked_until: Option<Instant>,
}
static RATE: Mutex<Rate> = Mutex::new(Rate {
    last: None,
    blocked_until: None,
});
fn blocked(message: &str) -> Value {
    json!({"state":"blocked","error":message,"retryAfterSeconds":60})
}
fn failure(message: &str) -> Value {
    json!({"state":"failed","error":message})
}
pub(super) trait Fetcher {
    fn get(&mut self, url: &str) -> Value;
}
struct Http;
impl Fetcher for Http {
    fn get(&mut self, url: &str) -> Value {
        {
            let mut rate = RATE.lock().unwrap_or_else(|e| e.into_inner());
            if rate.blocked_until.is_some_and(|v| v > Instant::now()) {
                return blocked("豆瓣访问暂受限，请稍后重试");
            }
            if let Some(last) = rate.last {
                let delay = Duration::from_secs(2).saturating_sub(last.elapsed());
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                }
            }
            rate.last = Some(Instant::now());
        }
        let response=(||{
            let client=reqwest::blocking::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(12)).user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36").build().map_err(|_|())?;
            let mut response=client.get(url).header("Accept-Language","zh-CN,zh;q=0.9").send().map_err(|_|())?;
            let status=response.status();let location=response.headers().get("location").and_then(|v|v.to_str().ok()).unwrap_or("").to_string();
            if status.as_u16()==403||status.as_u16()==429||location.contains("sec.douban.com"){return Ok(blocked("豆瓣返回安全验证或访问限流，未继续请求"));}
            if !status.is_success(){return Ok(failure(&format!("豆瓣返回 HTTP {}",status.as_u16())));}
            let mut bytes=Vec::new();response.by_ref().take(2*1024*1024+1).read_to_end(&mut bytes).map_err(|_|())?;
            if bytes.len()>2*1024*1024{return Ok(failure("豆瓣响应超过大小限制"));}
            let html=String::from_utf8(bytes).map_err(|_|())?;
            if verification(&html){return Ok(blocked("豆瓣要求安全验证，未解析为图书资料"));}
            Ok(json!({"state":"ok","body":html}))
        })().unwrap_or_else(|_:()|failure("请求豆瓣超时或网络不可用"));
        if response["state"] == "blocked" {
            RATE.lock().unwrap_or_else(|e| e.into_inner()).blocked_until =
                Some(Instant::now() + Duration::from_secs(60));
        }
        response
    }
}
fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}
fn plain(html: &str) -> String {
    let text = re("(?is)<[^>]*>")
        .replace_all(html, " ")
        .into_owned()
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    re(r"&#(x[0-9a-fA-F]+|[0-9]+);")
        .replace_all(&text, |c: &regex::Captures| {
            let n = if let Some(hex) = c[1].strip_prefix('x') {
                u32::from_str_radix(hex, 16).ok()
            } else {
                c[1].parse().ok()
            };
            n.and_then(char::from_u32).unwrap_or(' ').to_string()
        })
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn verification(html: &str) -> bool {
    html.contains("检测到有异常请求")
        || html.contains("请输入验证码")
        || re("(?is)<title[^>]*>[^<]*(安全验证|访问受限|访问异常)").is_match(html)
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("").trim()
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
}
fn subject_url(id: &str) -> String {
    format!("{ORIGIN}/subject/{id}/")
}
fn field(info: &str, label: &str) -> String {
    let pattern = format!(
        r"(?is){}\s*[:：]?\s*</span>\s*[:：]?\s*(.*?)(?:<br\s*/?>|$)",
        regex::escape(label)
    );
    re(&pattern)
        .captures(info)
        .map(|c| plain(&c[1]))
        .unwrap_or_default()
}
fn meta(html: &str, key: &str) -> String {
    let attributes = re(r#"(?is)([\w:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#);
    for tag in re("(?is)<meta\\b[^>]*>").find_iter(html) {
        let mut name = String::new();
        let mut value = String::new();
        for a in attributes.captures_iter(tag.as_str()) {
            let content = a.get(2).or_else(|| a.get(3)).unwrap().as_str();
            if ["property", "name", "itemprop"].contains(&&a[1].to_lowercase()[..]) {
                name = content.to_string();
            }
            if a[1].eq_ignore_ascii_case("content") {
                value = plain(content);
            }
        }
        if name.eq_ignore_ascii_case(key) {
            return value;
        }
    }
    String::new()
}
fn normalized_isbn(raw: &str) -> Option<String> {
    super::imports::isbn(&plain(raw))
        .ok()
        .filter(|s| !s.is_empty())
}
fn json_isbn(v: &Value, depth: usize, title: &str) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let own_book = v["@type"] == "Book"
        && !title.is_empty()
        && plain(v["name"].as_str().unwrap_or("")) == title;
    (if own_book {
        normalized_isbn(v["isbn"].as_str().unwrap_or(""))
    } else {
        None
    })
    .or_else(|| {
        v.as_array().and_then(|a| {
            a.iter()
                .take(20)
                .find_map(|v| json_isbn(v, depth + 1, title))
        })
    })
    .or_else(|| v.get("@graph").and_then(|v| json_isbn(v, depth + 1, title)))
}
pub(super) fn detail(html: &str, id: &str) -> Value {
    if verification(html) {
        return blocked("豆瓣要求安全验证");
    }
    let info = re(r#"(?is)<div\b[^>]*\bid\s*=\s*["']info["'][^>]*>(.*?)</div>"#)
        .captures(html)
        .map(|c| c[1].to_string())
        .unwrap_or_default();
    let title = meta(html, "og:title");
    let title = if title.is_empty() {
        re(r#"(?is)<span[^>]*property\s*=\s*["']v:itemreviewed["'][^>]*>(.*?)</span>"#)
            .captures(html)
            .map(|c| plain(&c[1]))
            .unwrap_or_default()
    } else {
        title
    };
    if title.is_empty() && info.is_empty() {
        return failure("响应不是可识别的豆瓣图书详情页");
    }
    let code = normalized_isbn(&field(&info, "ISBN"))
        .or_else(|| normalized_isbn(&meta(html, "books:isbn")))
        .or_else(|| normalized_isbn(&meta(html, "isbn")))
        .or_else(|| {
            re(r#"(?is)<script\b[^>]*type\s*=\s*["']application/ld\+json["'][^>]*>(.*?)</script>"#)
                .captures_iter(html)
                .filter_map(|c| serde_json::from_str::<Value>(&c[1]).ok())
                .find_map(|v| json_isbn(&v, 0, &title))
        });
    let year = field(&info, "出版年");
    let year = if year.len() >= 4 && year.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        &year[..4]
    } else {
        ""
    };
    json!({"state":if code.is_some(){"found"}else{"isbn_missing"},"item":{"subjectId":id,"title":title,"author":field(&info,"作者"),"publisher":field(&info,"出版社"),"publicationYear":year,"edition":field(&info,"版次"),"isbn":code,"coverUrl":meta(html,"og:image"),"sourceUrl":subject_url(id)}})
}
fn suggestions(body: &str) -> Result<Value> {
    let raw: Value = serde_json::from_str(body).map_err(|_| "豆瓣建议接口未返回 JSON 列表")?;
    let list = raw.as_array().ok_or("豆瓣建议接口返回格式不符")?;
    let mut items = Vec::new();
    for item in list.iter().take(20) {
        let id = item["id"]
            .as_str()
            .map(str::to_string)
            .or_else(|| item["id"].as_u64().map(|v| v.to_string()))
            .unwrap_or_default();
        let title = plain(text(item, "title"));
        if !valid_id(&id) || title.is_empty() {
            continue;
        }
        let author = if text(item, "author_name").is_empty() {
            text(item, "author")
        } else {
            text(item, "author_name")
        };
        let cover = text(item, "pic");
        let cover = reqwest::Url::parse(cover)
            .ok()
            .filter(|u| {
                ["https", "http"].contains(&u.scheme())
                    && u.host_str().is_some()
                    && u.username().is_empty()
                    && u.password().is_none()
            })
            .map(|u| u.to_string())
            .unwrap_or_default();
        items.push(json!({"subjectId":id,"title":title,"author":plain(author),"publicationYear":text(item,"year"),"coverUrl":cover,"isbn":Value::Null,"sourceUrl":subject_url(&id)}));
    }
    if !list.is_empty() && items.is_empty() {
        return Err("豆瓣建议列表缺少有效图书编号或标题".into());
    }
    Ok(json!({"state":if items.is_empty(){"not_found"}else{"candidates"},"items":items}))
}
fn cache(
    s: &AppState,
    key: &str,
    url: &str,
    fetch: &mut impl Fetcher,
    parse: impl FnOnce(&str) -> Result<Value>,
    ttl: i64,
) -> Result<Value> {
    let now = chrono::Utc::now().timestamp();
    let prior: Option<String> = lock(s)
        .query_row(
            "SELECT data FROM agent_isbn_cache WHERE cache_key=? AND expires_at>?",
            params![key, now],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(raw) = prior {
        let mut v: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        v["cached"] = json!(true);
        return Ok(v);
    }
    let response = fetch.get(url);
    let mut result = if response["state"] == "ok" {
        parse(text(&response, "body")).unwrap_or_else(|e| failure(&e))
    } else {
        response
    };
    result["cached"] = json!(false);
    result["fetchedAt"] = json!(db::now());
    let ttl = match text(&result, "state") {
        "blocked" => 60,
        "failed" => 15,
        "not_found" => 3600,
        _ => ttl,
    };
    lock(s).execute("INSERT INTO agent_isbn_cache(cache_key,data,expires_at,updated_at) VALUES(?,?,?,?) ON CONFLICT(cache_key) DO UPDATE SET data=excluded.data,expires_at=excluded.expires_at,updated_at=excluded.updated_at",params![key,result.to_string(),now+ttl,db::now()]).map_err(|e|e.to_string())?;
    Ok(result)
}
pub(super) fn lookup(s: &AppState, v: &Value) -> Result<Value> {
    lookup_with(s, v, &mut Http)
}
pub(super) fn lookup_with(s: &AppState, v: &Value, fetch: &mut impl Fetcher) -> Result<Value> {
    let fields = v.as_object().ok_or("ISBN 查询参数应为对象")?;
    if fields
        .keys()
        .any(|k| !["query", "subjectId"].contains(&k.as_str()))
        || fields.values().any(|v| !v.is_string())
    {
        return Err("仅支持文本 query 和 subjectId".into());
    }
    let query = text(v, "query")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let id = text(v, "subjectId");
    if query.len() > 600 {
        return Err("查询书名过长".into());
    }
    if !id.is_empty() {
        if !valid_id(id) {
            return Err("豆瓣 subjectId 须为数字编号".into());
        }
        return cache(
            s,
            &format!("subject:{id}"),
            &subject_url(id),
            fetch,
            |html| Ok(detail(html, id)),
            30 * 86400,
        );
    }
    if query.is_empty() {
        return Err("请提供书名 query 或豆瓣 subjectId".into());
    }
    let mut url = reqwest::Url::parse(&format!("{ORIGIN}/j/subject_suggest")).unwrap();
    url.query_pairs_mut().append_pair("q", &query);
    let mut result = cache(
        s,
        &format!("query:{query}"),
        url.as_str(),
        fetch,
        suggestions,
        7 * 86400,
    )?;
    result["query"] = json!(query);
    if result["state"] != "candidates" {
        return Ok(result);
    }
    let exact = result["items"].as_array().unwrap().iter().position(|item| {
        text(item, "title").split_whitespace().collect::<String>()
            == query.split_whitespace().collect::<String>()
    });
    if let Some(index) = exact {
        let id = text(&result["items"][index], "subjectId").to_string();
        let details = cache(
            s,
            &format!("subject:{id}"),
            &subject_url(&id),
            fetch,
            |html| Ok(detail(html, &id)),
            30 * 86400,
        )?;
        result["queryCached"] = result["cached"].clone();
        result["cached"] = json!(result["cached"] == true && details["cached"] == true);
        if details["item"].is_object() {
            let target = &mut result["items"][index];
            for (key, value) in details["item"].as_object().unwrap() {
                if value.as_str() != Some("") {
                    target[key] = value.clone();
                }
            }
            target["detailState"] = details["state"].clone();
            target["detailCached"] = details["cached"].clone();
            target["fetchedAt"] = details["fetchedAt"].clone();
        } else {
            result["detailError"] = details;
        }
    }
    result["selectionRequired"] = json!(true);
    result["note"] = json!("候选仅供核对；用 subjectId 查询其他版本详情，不自动写入书库");
    Ok(result)
}
