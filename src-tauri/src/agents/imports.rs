use crate::{
    db, images,
    models::BookPatch,
    state::{lock, AppState},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("").trim()
}
fn required(v: &Value, k: &str) -> Result<String> {
    let s = text(v, k);
    if s.is_empty() || s.len() > 3000 {
        return Err(format!("{k} 缺失或过长"));
    }
    Ok(s.into())
}
fn web(v: &Value, k: &str) -> Result<String> {
    let raw = required(v, k)?;
    let u = reqwest::Url::parse(&raw).map_err(err)?;
    if !["https", "http"].contains(&u.scheme())
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err(format!("{k} 须为公开 HTTP(S) 来源"));
    }
    Ok(u.to_string())
}
fn book(c: &Connection, id: i64) -> Result<()> {
    if db::get_book(c, id).map_err(err)?.is_none() {
        return Err("图书不存在".into());
    }
    Ok(())
}
fn bid(v: &Value) -> Result<i64> {
    v["bookId"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or("请提供有效 bookId".into())
}
fn identity(
    c: &Connection,
    tool: &str,
    v: &Value,
) -> Result<(String, String, String, Option<Value>)> {
    let source = required(v, "source")?;
    let external = required(v, "externalId")?;
    if source.len() > 100 || external.len() > 200 {
        return Err("来源或外部编号过长".into());
    }
    let hash = format!("{:x}", md5::compute(v.to_string()));
    let prior:Option<(String,String)>=c.query_row("SELECT payload_hash,result FROM agent_imports WHERE tool=? AND source=? AND external_id=?",params![tool,source,external],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(err)?;
    let prior = match prior {
        Some((h, result)) => {
            if h != hash {
                return Err("该外部编号已导入不同内容，请核对来源或使用新的修订编号".into());
            }
            let mut r: Value = serde_json::from_str(&result).map_err(err)?;
            if let Some(id) = r["bookId"].as_i64() {
                if db::get_book(c, id).map_err(err)?.is_none() {
                    return Err("原导入图书已删除；如需重新入库，请使用新的修订编号".into());
                }
            }
            r["reused"] = json!(true);
            Some(r)
        }
        None => None,
    };
    Ok((source, external, hash, prior))
}
fn remember(
    c: &Connection,
    tool: &str,
    key: &(String, String, String, Option<Value>),
    result: &Value,
    source_url: &str,
) -> Result<()> {
    c.execute("INSERT INTO agent_imports(tool,source,external_id,payload_hash,result,source_url,created_at) VALUES(?,?,?,?,?,?,?)",params![tool,key.0,key.1,key.2,result.to_string(),source_url,db::now()]).map_err(err)?;
    Ok(())
}
pub(super) fn isbn(s: &str) -> Result<String> {
    let s = s.replace([' ', '-'], "").to_uppercase();
    if s.is_empty() {
        return Ok(s);
    }
    let valid = if s.len() == 13
        && (s.starts_with("978") || s.starts_with("979"))
        && s.bytes().all(|b| b.is_ascii_digit())
    {
        s.bytes()
            .enumerate()
            .map(|(i, b)| (b - b'0') as u32 * if i % 2 == 0 { 1 } else { 3 })
            .sum::<u32>()
            % 10
            == 0
    } else if s.len() == 10 {
        s.bytes()
            .enumerate()
            .map(|(i, b)| {
                if b.is_ascii_digit() {
                    Some((10 - i) as u32 * (b - b'0') as u32)
                } else if i == 9 && b == b'X' {
                    Some(10)
                } else {
                    None
                }
            })
            .collect::<Option<Vec<_>>>()
            .is_some_and(|v| v.iter().sum::<u32>() % 11 == 0)
    } else {
        false
    };
    if valid {
        Ok(s)
    } else {
        Err("ISBN 校验失败".into())
    }
}
fn profile(c: &Connection, id: i64) -> Result<Value> {
    let raw: Option<String> = c
        .query_row(
            "SELECT data FROM book_profiles WHERE book_id=?",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    Ok(raw
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or(json!({})))
}
fn save_profile(c: &Connection, id: i64, v: &Value) -> Result<()> {
    c.execute("INSERT INTO book_profiles(book_id,data) VALUES(?,?) ON CONFLICT(book_id) DO UPDATE SET data=excluded.data",params![id,v.to_string()]).map_err(err)?;
    Ok(())
}
pub(super) fn execute(s: &AppState, tool: &str, v: &Value) -> Result<Value> {
    if tool=="taoshu_generate_draft"{return super::ai::mcp_request(s,v);}
    if tool == "taoshu_import_books" {
        return batch(s, v);
    }
    if tool == "taoshu_lookup_isbn" {
        return super::isbn::lookup(s, v);
    }
    if tool == "taoshu_context" {
        return Ok(
            json!({"application":"淘书工坊","version":env!("CARGO_PKG_VERSION"),"importTypes":["book","schoolBook","source","image"],"lookup":"taoshu_lookup_isbn 按书名查豆瓣候选，按 subjectId 获取指定版本 ISBN；不自动修改书库","ai":"taoshu_generate_draft 创建 AI 定价、文案或文本任务并查询结果；自动写入本地草稿，不选择货源或发布","costRule":"淘宝有效商品价按不同价位去重，取第二低价，不含运费；不足两档则待补充","identity":"每次导入提供 source、externalId；重试保留原参数","limits":{"imageBytes":10485760,"searchResults":50,"batchBooks":200,"aiBooks":100},"scope":"图书资料查询、导入及本地 AI 草稿；不提供订单、支付、发布、发货或密钥接口"}),
        );
    }
    if tool == "taoshu_search_books" {
        let q = text(v, "query");
        if q.len() > 200 {
            return Err("检索词过长".into());
        }
        let c = lock(s);
        let mut stmt=c.prepare("SELECT id,title,author,publisher,isbn,edition FROM books WHERE title LIKE ?1 OR isbn LIKE ?1 OR author LIKE ?1 ORDER BY id DESC LIMIT 50").map_err(err)?;
        let rows=stmt.query_map([format!("%{q}%")],|r|Ok(json!({"bookId":r.get::<_,i64>(0)?,"title":r.get::<_,String>(1)?,"author":r.get::<_,String>(2)?,"publisher":r.get::<_,String>(3)?,"isbn":r.get::<_,String>(4)?,"edition":r.get::<_,String>(5)?}))).map_err(err)?.collect::<std::result::Result<Vec<_>,_>>().map_err(err)?;
        let mut rows = rows;
        for row in &mut rows {
            let p = profile(&c, row["bookId"].as_i64().unwrap())?;
            row["volume"] = p["volume"].clone();
        }
        return Ok(json!({"items":rows,"limit":50}));
    }
    if ![
        "taoshu_import_book",
        "taoshu_import_source",
        "taoshu_import_school_book",
        "taoshu_import_image",
    ]
    .contains(&tool)
    {
        return Err("未提供该业务工具".into());
    }
    let mut c = lock(s);
    let tx = c.transaction().map_err(err)?;
    let key = identity(&tx, tool, v)?;
    if let Some(ref prior) = key.3 {
        return Ok(prior.clone());
    }
    let source_url = web(v, "sourceUrl")?;
    let result = match tool {
        "taoshu_import_book" => {
            let title = required(v, "title")?;
            let code = isbn(text(v, "isbn"))?;
            let edition = text(v, "edition");
            let volume = text(v, "volume");
            let author = text(v, "author");
            let publisher = text(v, "publisher");
            if edition.is_empty() {
                return Err("请提供来源明确的版次；未知版次不能自动建档".into());
            }
            let candidates: Vec<i64> = {
                let mut stmt=tx.prepare("SELECT id FROM books WHERE (?1<>'' AND isbn=?1) OR (?1='' AND title=?2 AND author=?3 AND publisher=?4)").map_err(err)?;
                let values = stmt
                    .query_map(params![code, title, author, publisher], |r| r.get(0))
                    .map_err(err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(err)?;
                values
            };
            let mut matches = Vec::new();
            for id in candidates {
                let old = db::get_book(&tx, id).map_err(err)?.unwrap();
                let p = profile(&tx, id)?;
                if code.is_empty() && (author.is_empty() || publisher.is_empty()) {
                    continue;
                }
                if old.edition == edition && text(&p, "volume") == volume {
                    if old.title != title
                        || (!author.is_empty() && !old.author.is_empty() && old.author != author)
                        || (!publisher.is_empty()
                            && !old.publisher.is_empty()
                            && old.publisher != publisher)
                    {
                        return Err(format!(
                            "现有 ISBN/版本的书目资料冲突（bookId={id}），请先核对"
                        ));
                    }
                    matches.push(id);
                }
            }
            if matches.len() > 1 {
                return Err("同一版本已有多份记录，请先在系统中核对".into());
            }
            let created = matches.is_empty();
            let id = if let Some(id) = matches.first() {
                *id
            } else {
                let patch:BookPatch=serde_json::from_value(json!({"title":title,"author":author,"publisher":publisher,"isbn":code,"edition":edition,"stock":0})).map_err(err)?;
                db::create_book(&tx, &patch).map_err(err)?.id
            };
            let mut p = profile(&tx, id)?;
            p["volume"] = json!(volume);
            if !text(v, "publicationYear").is_empty() {
                let year = text(v, "publicationYear");
                if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("出版年份应为四位数字".into());
                }
                if !text(&p, "publicationYear").is_empty() && p["publicationYear"] != year {
                    return Err("现有出版年份冲突".into());
                }
                p["publicationYear"] = json!(year);
            }
            if created {
                p["confirmed"] = json!(false);
            }
            let mut evidence = p["agentEvidence"].as_array().cloned().unwrap_or_default();
            evidence.push(
                json!({"source":key.0,"externalId":key.1,"url":source_url,"importedAt":db::now()}),
            );
            p["agentEvidence"] = json!(evidence);
            save_profile(&tx, id, &p)?;
            json!({"bookId":id,"created":created,"reused":false})
        }
        "taoshu_import_source" => {
            let id = bid(v)?;
            book(&tx, id)?;
            let url = web(v, "url")?;
            for field in ["price", "freight"] {
                if !v[field].is_null()
                    && !v[field]
                        .as_f64()
                        .is_some_and(|n| n.is_finite() && n >= 0.0 && n <= 1_000_000.0)
                {
                    return Err(format!("{field} 无效；未知金额使用 null"));
                }
            }
            let existing:Option<i64>=tx.query_row("SELECT id FROM book_sources WHERE book_id=? AND json_extract(data,'$.url')=? ORDER BY id LIMIT 1",params![id,url],|r|r.get(0)).optional().map_err(err)?;
            let sid = if let Some(sid) = existing {
                sid
            } else {
                let data = json!({"url":url,"title":text(v,"title"),"shop":text(v,"shop"),"sku":text(v,"sku"),"condition":text(v,"condition"),"price":v["price"],"freight":v["freight"],"status":"pending","dropship":false,"checkedAt":"","origin":"agent","sourceUrl":source_url,"agentSource":key.0});
                tx.execute(
                    "INSERT INTO book_sources(book_id,is_primary,data,updated_at) VALUES(?,0,?,?)",
                    params![id, data.to_string(), db::now()],
                )
                .map_err(err)?;
                tx.last_insert_rowid()
            };
            json!({"bookId":id,"sourceId":sid,"existingSource":existing.is_some(),"reused":false})
        }
        "taoshu_import_school_book" => {
            let id = bid(v)?;
            book(&tx, id)?;
            let school = required(v, "school")?;
            let major = required(v, "major")?;
            let raw = required(v, "raw")?;
            let year = v["year"]
                .as_i64()
                .filter(|n| (2000..=2200).contains(n))
                .ok_or("年份无效")?;
            let b = db::get_book(&tx, id).map_err(err)?.unwrap();
            let p = profile(&tx, id)?;
            if text(v, "edition") != b.edition || text(v, "volume") != text(&p, "volume") {
                return Err("学校引用的版次、册次与图书不一致".into());
            }
            let rid = format!("agent-{:x}", md5::compute(format!("{}:{}", key.0, key.1)));
            let data = json!({"id":rid,"school":school,"year":year,"major":major,"college":text(v,"college"),"majorCode":text(v,"majorCode"),"subject":text(v,"subject"),"stage":text(v,"stage"),"raw":raw,"sourceUrl":source_url,"agentSource":key.0,"candidate":{"title":b.title,"isbn":b.isbn,"edition":b.edition,"volume":text(&p,"volume")}});
            tx.execute("INSERT INTO school_requirements(id,book_id,school,year,major,state,data,updated_at) VALUES(?,?,?,?,?,'confirmed',?,?)",params![rid,id,school,year,major,data.to_string(),db::now()]).map_err(err)?;
            json!({"bookId":id,"requirementId":rid,"reused":false})
        }
        "taoshu_import_image" => {
            let id = bid(v)?;
            book(&tx, id)?;
            let raw = text(v, "base64");
            if raw.is_empty() || raw.len() > 14 * 1024 * 1024 {
                return Err("图片 Base64 缺失或超过限制".into());
            }
            let bytes = STANDARD.decode(raw).map_err(|_| "图片 Base64 无效")?;
            if bytes.len() > 10 * 1024 * 1024 {
                return Err("图片超过 10MB".into());
            }
            let format = image::guess_format(&bytes).map_err(err)?;
            let mime = match format {
                image::ImageFormat::Png => "image/png",
                image::ImageFormat::Jpeg => "image/jpeg",
                image::ImageFormat::WebP => "image/webp",
                _ => return Err("仅支持 PNG、JPEG、WebP".into()),
            };
            if text(v, "mimeType") != mime {
                return Err("图片格式与 mimeType 不一致".into());
            }
            let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(10000);
            limits.max_image_height = Some(10000);
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            reader.decode().map_err(err)?;
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT id FROM images WHERE book_id=? AND origin_url=? ORDER BY id LIMIT 1",
                    params![id, source_url],
                    |r| r.get(0),
                )
                .optional()
                .map_err(err)?;
            let image_id = if let Some(i) = existing {
                i
            } else {
                let saved = images::save_bytes(&bytes, mime, &s.image_dir, "agent").map_err(err)?;
                match db::add_image(
                    &tx,
                    id,
                    "agent",
                    &source_url,
                    &saved.file,
                    saved.width,
                    saved.height,
                ) {
                    Ok(i) => i.id,
                    Err(e) => {
                        images::remove_file(&s.image_dir, &saved.file);
                        return Err(err(e));
                    }
                }
            };
            json!({"bookId":id,"imageId":image_id,"existingImage":existing.is_some(),"reused":false})
        }
        _ => unreachable!(),
    };
    remember(&tx, tool, &key, &result, text(v, "sourceUrl"))?;
    if ["taoshu_import_source","taoshu_import_book"].contains(&tool){if let Some(id)=result["bookId"].as_i64(){super::ai::auto_queue(&tx,id,if tool=="taoshu_import_source"{"source"}else{"book"}).map_err(err)?;}}
    tx.commit().map_err(err)?;
    Ok(result)
}

pub(super) fn batch(s: &AppState, v: &Value) -> Result<Value> {
    let fields = v.as_object().ok_or("批量参数应为对象")?;
    if fields.keys().any(|k| k != "books") {
        return Err("批量参数仅支持 books".into());
    }
    let books = v["books"]
        .as_array()
        .filter(|b| !b.is_empty() && b.len() <= 200)
        .ok_or("每批须为 1–200 本图书")?;
    let mut rows = Vec::new();
    let (mut created, mut matched, mut reused, mut failed) = (0, 0, 0, 0);
    for (index, book) in books.iter().enumerate() {
        let result = (|| {
            let fields = book.as_object().ok_or("图书资料应为对象")?;
            for (key, value) in fields {
                if ![
                    "source",
                    "externalId",
                    "sourceUrl",
                    "title",
                    "author",
                    "publisher",
                    "isbn",
                    "edition",
                    "volume",
                    "publicationYear",
                ]
                .contains(&key.as_str())
                {
                    return Err(format!("不支持字段 {key}"));
                }
                if !value.is_string() || value.as_str().is_some_and(|v| v.len() > 3000) {
                    return Err(format!("{key} 须为不超过 3000 字节的文本"));
                }
            }
            execute(s, "taoshu_import_book", book)
        })();
        match result {
            Ok(result) => {
                let status = if result["reused"] == true {
                    reused += 1;
                    "reused"
                } else if result["created"] == true {
                    created += 1;
                    "created"
                } else {
                    matched += 1;
                    "matched"
                };
                rows.push(json!({"index":index+1,"externalId":book["externalId"],"status":status,"bookId":result["bookId"]}));
            }
            Err(error) => {
                failed += 1;
                rows.push(json!({"index":index+1,"externalId":book["externalId"],"status":"failed","error":error}));
            }
        }
    }
    Ok(
        json!({"total":books.len(),"success":created+matched+reused,"created":created,"matched":matched,"reused":reused,"failed":failed,"items":rows}),
    )
}
