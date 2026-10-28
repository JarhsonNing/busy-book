use super::*;

fn latest(c: &Connection, id: &str) -> Result<Value, ApiError> {
    let raw: Option<String> = c.query_row(
        "SELECT data FROM commerce_events WHERE entity='requirement' AND entity_id=? AND action='book_search' ORDER BY id DESC LIMIT 1",
        [id], |r| r.get(0)).optional()?;
    Ok(raw.map(parse).unwrap_or(json!({"state":"idle"})))
}
pub(super) async fn status(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(latest(&lock(&s), &id)?))
}
fn clean(v: &Value, key: &str) -> String {
    let mut tag = false;
    text(v, key)
        .chars()
        .filter(|ch| {
            if *ch == '<' {
                tag = true;
                return false;
            }
            if *ch == '>' {
                tag = false;
                return false;
            }
            !tag
        })
        .take(600)
        .collect::<String>()
        .trim()
        .to_string()
}
fn candidates(v: &Value) -> Vec<Value> {
    v.as_array().into_iter().flatten().take(30).filter_map(|b| {
        let title = clean(b,"title");
        let url = reqwest::Url::parse(text(b,"sourceUrl")).ok()?;
        let host = url.host_str()?;
        if title.is_empty() || url.scheme() != "https" || !(host == "kongfz.com" || host.ends_with(".kongfz.com")) { return None; }
        Some(json!({"title":title,"author":clean(b,"author"),"publisher":clean(b,"publisher"),"isbn":clean(b,"isbn"),
            "edition":clean(b,"edition"),"volume":clean(b,"volume"),"publicationYear":clean(b,"publicationYear"),"sourceUrl":url.as_str()}))
    }).collect()
}
pub(super) async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let raw: String = c
        .query_row(
            "SELECT data FROM school_requirements WHERE id=?",
            [&id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| ApiError::bad("用书记录不存在"))?;
    let result = if v["action"] == "start" {
        let r = parse(raw);
        let b = &r["candidate"];
        let query = if !text(&v, "query").trim().is_empty() {
            text(&v, "query")
                .trim()
                .chars()
                .take(200)
                .collect::<String>()
        } else if !text(b, "isbn").is_empty() {
            text(b, "isbn").to_string()
        } else {
            [
                text(b, "title"),
                b["authors"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                .or_else(|| b["authors"].as_str())
                .unwrap_or(""),
                text(b, "edition"),
                text(b, "volume"),
            ]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ")
        };
        if query.is_empty() {
            return Err(ApiError::bad("请填写检索词"));
        }
        let request_id = format!(
            "{}-{}",
            chrono::Utc::now().timestamp_millis(),
            rand::random::<u64>()
        );
        let mut url = reqwest::Url::parse("https://search.kongfz.com/product/").unwrap();
        url.query_pairs_mut()
            .append_pair("keyword", &query)
            .append_pair("taoshuRequirement", &id)
            .append_pair("taoshuLookup", &request_id);
        json!({"state":"searching","requestId":request_id,"query":query,"searchUrl":url.as_str(),"items":[],"updatedAt":db::now()})
    } else {
        let previous = latest(&c, &id)?;
        if text(&previous, "requestId").is_empty() || v["requestId"] != previous["requestId"] {
            return Err(ApiError::bad("检索已过期，请重新检索"));
        }
        let mut result = previous;
        result["items"] = json!(candidates(&v["items"]));
        result["state"] = json!(if text(&v, "error").is_empty() {
            "completed"
        } else {
            "failed"
        });
        result["error"] = json!(clean(&v, "error"));
        result["updatedAt"] = json!(db::now());
        result
    };
    event(&c, "requirement", &id, "book_search", &result)?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidates_keep_versions_and_reject_untrusted_links() {
        let v = candidates(&json!([
            {"title":"<b>数据库</b>","isbn":"9787040406641","edition":"第5版","sourceUrl":"https://item.kongfz.com/book/1"},
            {"title":"数据库","edition":"第6版","sourceUrl":"https://book.kongfz.com/2"},
            {"title":"假来源","sourceUrl":"https://kongfz.com.evil.test/"},
            {"title":"假来源","sourceUrl":"javascript:alert(1)"}
        ]));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0]["title"], "数据库");
        assert_eq!(v[0]["edition"], "第5版");
        assert_eq!(v[1]["edition"], "第6版");
    }
}
