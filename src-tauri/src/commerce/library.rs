use super::*;
use axum::extract::Query;
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Filter {
    #[serde(default)]
    q: String,
    #[serde(default)]
    school: String,
    #[serde(default)]
    major: String,
    year: Option<i64>,
    #[serde(default)]
    readiness: String,
    page: Option<i64>,
    page_size: Option<i64>,
}

pub(super) async fn list(
    State(s): State<AppState>,
    Query(f): Query<Filter>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let size = f.page_size.unwrap_or(30).clamp(1, 100);
    let search = format!("%{}%", f.q.trim());
    let major = format!("%{}%", f.major.trim());
    let clause=" WHERE (?1='' OR b.title LIKE ?2 OR b.author LIKE ?2 OR b.isbn LIKE ?2 OR b.publisher LIKE ?2)
      AND ((?3='' AND ?4='' AND ?5 IS NULL) OR EXISTS(SELECT 1 FROM school_requirements r WHERE r.book_id=b.id AND r.state='confirmed' AND (?3='' OR r.school=?3) AND (?4='' OR r.major LIKE ?6) AND (?5 IS NULL OR r.year=?5)))
      AND (?7<>'sources' OR NOT EXISTS(SELECT 1 FROM book_sources bs WHERE bs.book_id=b.id AND json_extract(bs.data,'$.status')='available' AND json_extract(bs.data,'$.dropship')=1))
      AND (?7<>'images' OR NOT EXISTS(SELECT 1 FROM images i WHERE i.book_id=b.id))
      AND (?7<>'version' OR b.isbn='' OR b.edition='')";
    let args = params![
        f.q.trim(),
        search,
        f.school,
        f.major,
        f.year,
        major,
        f.readiness
    ];
    let total: i64 = c.query_row(
        &format!("SELECT COUNT(*) FROM books b{clause}"),
        args,
        |r| r.get(0),
    )?;
    let page = f
        .page
        .unwrap_or(1)
        .max(1)
        .min(((total + size - 1) / size).max(1));
    let sql = format!(
        "SELECT b.id FROM books b{clause} ORDER BY b.id DESC LIMIT {size} OFFSET {}",
        (page - 1) * size
    );
    let ids = c
        .prepare(&sql)?
        .query_map(args, |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut items = Vec::new();
    for id in ids {
        if let Some(b) = db::get_book(&c, id)? {
            items.push(summary(&c, &b)?);
        }
    }
    let schools=rows(&c,"SELECT json_object('name',school) FROM school_requirements GROUP BY school ORDER BY school",[])?;
    let years = rows(
        &c,
        "SELECT json_object('year',year) FROM school_requirements GROUP BY year ORDER BY year DESC",
        [],
    )?;
    Ok(Json(
        json!({"items":items,"total":total,"page":page,"pageSize":size,"schools":schools,"years":years}),
    ))
}

pub(super) async fn sync_catalog(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let Some(path) = catalog_browser::resolve_path(&s) else {
        return Ok(Json(json!({"available":false})));
    };
    let path = path.with_extension("json");
    if !path.exists() {
        return Ok(Json(json!({"available":false})));
    }
    let raw = std::fs::read(&path)?;
    let digest = format!("v2:{:x}", md5::compute(&raw));
    if db::get_setting(&lock(&s), "catalogImportedDigest") == Some(json!(digest)) {
        return Ok(Json(json!({"available":true,"unchanged":true})));
    }
    let v: Value = serde_json::from_slice(&raw)?;
    let Json(mut result) = catalog::import(State(s.clone()), Json(v)).await?;
    db::set_setting(&lock(&s), "catalogImportedDigest", &json!(digest))?;
    result["available"] = json!(true);
    Ok(Json(result))
}
