//! Read-only browsing of the offline kaoyan catalog (`catalog.db`) along two dimensions:
//! school -> major -> textbook, and textbook -> the schools/majors that require it.
//! The catalog is never written here; it is produced offline by `scripts/kaoyan/build-catalog.mjs`.
use super::*;
use rusqlite::OpenFlags;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

const SETTING_KEY: &str = "kaoyanCatalogPath";

fn override_path(state: &AppState) -> Option<String> {
    let conn = lock(state);
    db::get_setting(&conn, SETTING_KEY)
        .and_then(|value| value.as_str().map(|text| text.trim().to_string()))
        .filter(|text| !text.is_empty())
}

// Compile-time repo root; only resolves when the app is built/run inside the source tree.
fn repo_default() -> Option<PathBuf> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| {
            root.join("data")
                .join("kaoyan")
                .join("catalog")
                .join("catalog.db")
        })
}

fn data_dir_candidate(state: &AppState) -> PathBuf {
    state.data_dir.join("kaoyan").join("catalog.db")
}

pub(super) fn resolve_path(state: &AppState) -> Option<PathBuf> {
    if let Some(path) = override_path(state) {
        return Some(PathBuf::from(path));
    }
    let candidate = data_dir_candidate(state);
    if candidate.exists() {
        return Some(candidate);
    }
    repo_default().filter(|path| path.exists())
}

fn try_open(path: &std::path::Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

fn require_catalog(state: &AppState) -> Result<Connection, ApiError> {
    let path = resolve_path(state).ok_or_else(|| {
        ApiError::bad(
            "尚未找到 catalog.db，请在下方指定路径，或先运行 npm run kaoyan:catalog 生成目录",
        )
    })?;
    if !path.exists() {
        return Err(ApiError::bad(format!(
            "目录数据库不存在：{}",
            path.display()
        )));
    }
    try_open(&path).map_err(|error| ApiError::bad(format!("打开目录数据库失败：{error}")))
}

fn one(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Value, ApiError> {
    conn.query_row(sql, params, |row| Ok(parse(row.get::<_, String>(0)?)))
        .optional()?
        .ok_or_else(|| ApiError::bad("记录不存在"))
}

fn empty_status() -> Value {
    json!({"imported":0,"confirmed":0,"bookIds":[],"physical":false,"digital":false,"listed":false})
}

// Cross-reference catalog textbooks against the commerce library. The join key is the catalog
// textbook id stored inside each imported requirement (`data.candidate.id`); missing keys simply
// degrade to "unlinked" rather than erroring.
fn book_status(
    state: &AppState,
    ids: &HashSet<String>,
) -> Result<HashMap<String, Value>, ApiError> {
    let mut map: HashMap<String, Value> = HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    for id in ids {
        map.insert(id.clone(), empty_status());
    }
    let conn = lock(state);
    let records = rows(
        &conn,
        "SELECT json_object('cid',json_extract(data,'$.candidate.id'),'state',state,'bookId',book_id) FROM school_requirements",
        [],
    )?;
    let mut linked: HashMap<String, HashSet<i64>> = HashMap::new();
    for record in &records {
        let cid = record["cid"].as_str().unwrap_or("");
        if cid.is_empty() || !ids.contains(cid) {
            continue;
        }
        if let Some(entry) = map.get_mut(cid) {
            entry["imported"] = json!(entry["imported"].as_i64().unwrap_or(0) + 1);
            if record["state"].as_str() == Some("confirmed") {
                entry["confirmed"] = json!(entry["confirmed"].as_i64().unwrap_or(0) + 1);
                if let Some(book_id) = record["bookId"].as_i64() {
                    linked.entry(cid.to_string()).or_default().insert(book_id);
                }
            }
        }
    }
    let mut flags: HashMap<i64, (bool, bool, bool)> = HashMap::new();
    for set in linked.values() {
        for &book_id in set {
            if flags.contains_key(&book_id) {
                continue;
            }
            if let Some(book) = db::get_book(&conn, book_id)? {
                let summary = summary(&conn, &book)?;
                flags.insert(
                    book_id,
                    (
                        summary["physicalReady"] == json!(true),
                        summary["digitalReady"] == json!(true),
                        summary["listed"] == json!(true),
                    ),
                );
            }
        }
    }
    for (cid, set) in &linked {
        if let Some(entry) = map.get_mut(cid) {
            let (mut physical, mut digital, mut listed) = (false, false, false);
            for book_id in set {
                if let Some(&(p, d, l)) = flags.get(book_id) {
                    physical |= p;
                    digital |= d;
                    listed |= l;
                }
            }
            let mut book_ids: Vec<i64> = set.iter().copied().collect();
            book_ids.sort_unstable();
            entry["bookIds"] = json!(book_ids);
            entry["physical"] = json!(physical);
            entry["digital"] = json!(digital);
            entry["listed"] = json!(listed);
        }
    }
    Ok(map)
}

fn attach_status(list: &mut [Value], status: &HashMap<String, Value>) {
    for item in list.iter_mut() {
        let id = item["id"].as_str().unwrap_or("");
        item["status"] = status.get(id).cloned().unwrap_or_else(empty_status);
    }
}

pub(super) async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let resolved = resolve_path(&state);
    let mut payload = json!({
        "configured": resolved.is_some(),
        "path": resolved.as_ref().map(|path| path.to_string_lossy().to_string()),
        "override": override_path(&state),
        "exists": resolved.as_ref().map(|path| path.exists()).unwrap_or(false),
        "dataDirCandidate": data_dir_candidate(&state).to_string_lossy().to_string(),
        "repoDefault": repo_default().map(|path| path.to_string_lossy().to_string()),
        "error": Value::Null,
        "stats": Value::Null,
    });
    if let Some(path) = resolved {
        match try_open(&path) {
            Ok(conn) => {
                let count = |table: &str| -> i64 {
                    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap_or(0)
                };
                let latest: Option<String> = conn
                    .query_row("SELECT MAX(collected_at) FROM sources", [], |row| {
                        row.get(0)
                    })
                    .optional()?;
                payload["stats"] = json!({
                    "schools": count("schools"),
                    "programs": count("programs"),
                    "subjects": count("subjects"),
                    "books": count("textbooks"),
                    "requirements": count("requirements"),
                    "sources": count("sources"),
                    "latestCollectedAt": latest,
                });
            }
            Err(error) => payload["error"] = json!(format!("打开目录数据库失败：{error}")),
        }
    }
    Ok(Json(payload))
}

pub(super) async fn set_path(
    State(state): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let path = text(&v, "path").trim().to_string();
    {
        let conn = lock(&state);
        db::set_setting(&conn, SETTING_KEY, &json!(path))?;
    }
    status(State(state)).await
}

pub(super) async fn tree(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = require_catalog(&state)?;
    let schools = rows(
        &conn,
        "SELECT json_object('id',id,'name',name,'programCount',program_count,'bookCount',book_count,'requirementCount',requirement_count) FROM schools ORDER BY name",
        [],
    )?;
    let programs = rows(
        &conn,
        "SELECT json_object('id',id,'schoolId',school_id,'year',year,'college',college,'majorCode',major_code,'majorName',major_name,'subjectCount',subject_count,'bookCount',book_count) FROM programs ORDER BY college, major_code, major_name",
        [],
    )?;
    let mut grouped: BTreeMap<String, BTreeMap<String, Vec<Value>>> = BTreeMap::new();
    for program in &programs {
        let school_id = program["schoolId"].as_str().unwrap_or("").to_string();
        let college = program["college"].as_str().unwrap_or("").to_string();
        grouped
            .entry(school_id)
            .or_default()
            .entry(college)
            .or_default()
            .push(program.clone());
    }
    let mut result: Vec<Value> = Vec::new();
    for school in &schools {
        let school_id = school["id"].as_str().unwrap_or("").to_string();
        let colleges = grouped.remove(&school_id).unwrap_or_default();
        let colleges: Vec<Value> = colleges
            .into_iter()
            .map(|(college, programs)| {
                json!({
                    "college": if college.is_empty() { Value::Null } else { json!(college) },
                    "programs": programs,
                })
            })
            .collect();
        let mut entry = school.clone();
        entry["colleges"] = json!(colleges);
        result.push(entry);
    }
    Ok(Json(json!({ "schools": result })))
}

pub(super) async fn major_books(
    State(state): State<AppState>,
    Path(program_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let conn = require_catalog(&state)?;
    let program = one(
        &conn,
        "SELECT json_object('id',p.id,'schoolId',p.school_id,'school',sc.name,'year',p.year,'college',p.college,'majorCode',p.major_code,'majorName',p.major_name,'pageUrl',p.page_url) FROM programs p JOIN schools sc ON sc.id=p.school_id WHERE p.id=?",
        [&program_id],
    )?;
    let subjects = rows(
        &conn,
        "SELECT json_object('id',id,'stage',stage,'code',code,'name',name,'raw',raw,'bookCount',book_count) FROM subjects WHERE program_id=? ORDER BY stage, code, name",
        [&program_id],
    )?;
    let mut books = rows(
        &conn,
        "SELECT json_object('id',t.id,'title',t.title,'authors',t.authors,'publisher',t.publisher,'edition',t.edition,'year',t.year,'subjectId',sub.id,'raw',r.raw,'confidence',r.confidence,'sourceUrl',r.source_url) FROM requirements r JOIN subjects sub ON sub.id=r.subject_id JOIN textbooks t ON t.id=r.textbook_id WHERE sub.program_id=? ORDER BY t.title",
        [&program_id],
    )?;
    let ids: HashSet<String> = books
        .iter()
        .filter_map(|book| book["id"].as_str().map(str::to_string))
        .collect();
    let status = book_status(&state, &ids)?;
    attach_status(&mut books, &status);

    let mut by_subject: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for book in books {
        let subject_id = book["subjectId"].as_str().unwrap_or("").to_string();
        by_subject.entry(subject_id).or_default().push(book);
    }
    let subjects: Vec<Value> = subjects
        .into_iter()
        .map(|mut subject| {
            let subject_id = subject["id"].as_str().unwrap_or("").to_string();
            subject["books"] = json!(by_subject.remove(&subject_id).unwrap_or_default());
            subject
        })
        .collect();
    Ok(Json(json!({ "program": program, "subjects": subjects })))
}

pub(super) async fn books(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = require_catalog(&state)?;
    let mut list = rows(
        &conn,
        "SELECT json_object('id',id,'title',title,'authors',authors,'publisher',publisher,'edition',edition,'year',year,'schoolCount',school_count,'programCount',program_count,'requirementCount',requirement_count) FROM textbooks ORDER BY requirement_count DESC, title",
        [],
    )?;
    let school_rows = rows(
        &conn,
        "SELECT json_object('id',textbook_id,'school',school) FROM (SELECT DISTINCT textbook_id, school FROM v_textbook_usage)",
        [],
    )?;
    let mut schools: HashMap<String, Vec<String>> = HashMap::new();
    for row in &school_rows {
        if let (Some(id), Some(school)) = (row["id"].as_str(), row["school"].as_str()) {
            schools
                .entry(id.to_string())
                .or_default()
                .push(school.to_string());
        }
    }
    let ids: HashSet<String> = list
        .iter()
        .filter_map(|book| book["id"].as_str().map(str::to_string))
        .collect();
    let status = book_status(&state, &ids)?;
    attach_status(&mut list, &status);
    for book in list.iter_mut() {
        let id = book["id"].as_str().unwrap_or("");
        book["schools"] = json!(schools.get(id).cloned().unwrap_or_default());
    }
    Ok(Json(json!({ "books": list, "count": list.len() })))
}

pub(super) async fn book_usage(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let conn = require_catalog(&state)?;
    let mut book = one(
        &conn,
        "SELECT json_object('id',id,'title',title,'authors',authors,'publisher',publisher,'edition',edition,'year',year,'schoolCount',school_count,'programCount',program_count,'requirementCount',requirement_count) FROM textbooks WHERE id=?",
        [&id],
    )?;
    let usage = rows(
        &conn,
        "SELECT json_object('school',school,'year',year,'college',college,'majorCode',major_code,'majorName',major_name,'stage',stage,'subjectCode',subject_code,'subjectName',subject_name,'confidence',confidence,'sourceUrl',source_url) FROM v_textbook_usage WHERE textbook_id=? ORDER BY school, year DESC, college, major_code, major_name",
        [&id],
    )?;
    let status = book_status(&state, &HashSet::from([id.clone()]))?;
    book["status"] = status.get(&id).cloned().unwrap_or_else(empty_status);
    Ok(Json(json!({ "book": book, "usage": usage })))
}
