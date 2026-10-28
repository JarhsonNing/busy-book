use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};

fn same_citation(a: &Value, b: &Value) -> bool {
    // Collection timestamps and aggregated candidates are evidence, not the identity of this citation.
    [
        "id",
        "school",
        "year",
        "college",
        "major",
        "majorCode",
        "stage",
        "subject",
        "subjectCode",
        "raw",
    ]
    .iter()
    .all(|key| a[*key] == b[*key])
}
fn index(v: &Value, key: &str) -> Result<HashMap<String, Value>, ApiError> {
    let list = v[key]
        .as_array()
        .ok_or_else(|| ApiError::bad(format!("目录缺少 {key} 数组")))?;
    let mut map = HashMap::new();
    for item in list {
        let id = required(item, "id")?;
        if map.insert(id, item.clone()).is_some() {
            return Err(ApiError::bad(format!("{key} 存在重复 ID")));
        }
    }
    Ok(map)
}
fn lookup<'a>(map: &'a HashMap<String, Value>, id: &str) -> Result<&'a Value, ApiError> {
    map.get(id)
        .ok_or_else(|| ApiError::bad(format!("目录引用不存在：{id}")))
}
pub(super) fn flatten(v: &Value) -> Result<Vec<Value>, ApiError> {
    let schools = index(v, "schools")?;
    let programs = index(v, "programs")?;
    let subjects = index(v, "subjects")?;
    let books = index(v, "books")?;
    let sources = index(v, "sources")?;
    let reqs = v["requirements"]
        .as_array()
        .ok_or_else(|| ApiError::bad("缺少 requirements 数组"))?;
    if reqs.len() > 50000 {
        return Err(ApiError::bad("单次最多导入 50000 条用书记录"));
    }
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for r in reqs {
        let sub = lookup(&subjects, text(r, "subjectId"))?;
        let p = lookup(&programs, text(sub, "programId"))?;
        let school = lookup(&schools, text(p, "schoolId"))?;
        let b = lookup(&books, text(r, "textbookId"))?;
        let source = lookup(&sources, text(p, "sourceId"))?;
        let year = p["year"]
            .as_i64()
            .ok_or_else(|| ApiError::bad("招生年份无效"))?;
        if !(2000..=2200).contains(&year) {
            return Err(ApiError::bad("招生年份无效"));
        }
        let scope = json!([
            text(school, "name"),
            year,
            text(p, "college"),
            text(p, "majorCode"),
            text(p, "majorName"),
            text(sub, "stage"),
            text(sub, "code"),
            text(sub, "name"),
            text(r, "id")
        ]);
        let id = format!("rq_{:x}", md5::compute(scope.to_string()));
        if !seen.insert(id.clone()) {
            return Err(ApiError::bad("目录存在重复用书记录"));
        }
        let ambiguous = b["editions"]
            .as_array()
            .map(|a| a.len() > 1)
            .unwrap_or(false)
            || b["volumes"]
                .as_array()
                .map(|a| a.len() > 1)
                .unwrap_or(false);
        result.push(json!({"id":id,"school":text(school,"name"),"year":year,"college":text(p,"college"),"major":text(p,"majorName"),"majorCode":text(p,"majorCode"),
            "stage":text(sub,"stage"),"subject":text(sub,"name"),"subjectCode":text(sub,"code"),"raw":required(r,"raw")?,"sourceUrl":text(r,"sourceUrl"),"sourceFile":text(r,"sourceFile"),"collectedAt":text(source,"collectedAt"),"candidate":b,"ambiguous":ambiguous}));
    }
    Ok(result)
}
pub(super) async fn preview(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let list = flatten(&v)?;
    let c = lock(&s);
    let mut report = Vec::new();
    for mut r in list {
        let existing: Option<String> = c
            .query_row(
                "SELECT data FROM school_requirements WHERE id=?",
                [text(&r, "id")],
                |row| row.get(0),
            )
            .optional()?;
        r["importAction"] = json!(match existing {
            None => "new",
            Some(raw) =>
                if same_citation(&parse(raw), &r) {
                    "unchanged"
                } else {
                    "changed"
                },
        });
        report.push(r)
    }
    Ok(Json(json!({"records":report,"count":report.len()})))
}
pub(super) async fn import(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let records = flatten(&v)?;
    let mut c = lock(&s);
    let tx = c.transaction()?;
    let schools = index(&v, "schools")?;
    for program in v["programs"].as_array().unwrap() {
        let school = lookup(&schools, text(program, "schoolId"))?;
        let data = json!({"school":school["name"],"year":program["year"],"college":program["college"],"major":program["majorName"],"majorCode":program["majorCode"]});
        let identity = format!("sp_{:x}", md5::compute(data.to_string()));
        tx.execute("INSERT INTO school_programs(id,data) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![identity,data.to_string()])?;
    }
    let mut counts = [0, 0, 0];
    for mut r in records {
        // Old collectors grouped editions into one candidate. Preserve the confirmed
        // citation ID when only that candidate identifier changed; raw evidence must match.
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM school_requirements WHERE id=?)",
            [text(&r, "id")],
            |row| row.get(0),
        )?;
        if !exists {
            let old_rows = rows(
                &tx,
                "SELECT data FROM school_requirements WHERE school=? AND year=? AND major=?",
                params![text(&r, "school"), r["year"].as_i64(), text(&r, "major")],
            )?;
            let matches: Vec<_> = old_rows
                .iter()
                .filter(|old| {
                    let mut candidate = r.clone();
                    candidate["id"] = old["id"].clone();
                    same_citation(old, &candidate)
                })
                .collect();
            if matches.len() == 1 {
                r["id"] = matches[0]["id"].clone();
            }
        }
        let id = text(&r, "id");
        let old: Option<String> = tx
            .query_row(
                "SELECT data FROM school_requirements WHERE id=?",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if old
            .as_ref()
            .map(|raw| same_citation(&parse(raw.clone()), &r))
            .unwrap_or(false)
        {
            tx.execute(
                "UPDATE school_requirements SET data=? WHERE id=?",
                params![r.to_string(), id],
            )?;
            counts[2] += 1;
            continue;
        }
        let changed = old.is_some();
        counts[if changed { 1 } else { 0 }] += 1;
        // Any changed citation requires a fresh confirmation; historical order/book data is untouched.
        tx.execute("INSERT INTO school_requirements(id,school,year,major,data,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data,book_id=NULL,state='pending',updated_at=excluded.updated_at",params![id,text(&r,"school"),r["year"].as_i64(),text(&r,"major"),r.to_string(),db::now()])?;
        if changed {
            event(
                &tx,
                "requirement",
                id,
                "source_changed",
                &json!({"before":old.map(parse)}),
            )?;
        }
    }
    tx.commit()?;
    Ok(Json(
        json!({"added":counts[0],"changed":counts[1],"unchanged":counts[2]}),
    ))
}
pub(super) async fn list(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let records=rows(&c,"SELECT json_object('id',id,'bookId',book_id,'state',state,'data',json(data),'updatedAt',updated_at) FROM school_requirements ORDER BY school,year DESC,major,id",[])?;
    let mut schools: BTreeMap<String, Value> = BTreeMap::new();
    for r in &records {
        let d = &r["data"];
        let key = format!("{}|{}", text(d, "school"), d["year"]);
        let sc=schools.entry(key).or_insert(json!({"school":d["school"],"year":d["year"],"total":0,"confirmed":0,"physical":0,"digital":0,"listed":0}));
        sc["total"] = json!(sc["total"].as_i64().unwrap() + 1);
        if let Some(bid) = r["bookId"].as_i64() {
            if r["state"] == "confirmed" {
                sc["confirmed"] = json!(sc["confirmed"].as_i64().unwrap() + 1);
                if let Some(b) = db::get_book(&c, bid)? {
                    let v = summary(&c, &b)?;
                    for (name, flag) in [
                        ("physical", "physicalReady"),
                        ("digital", "digitalReady"),
                        ("listed", "listed"),
                    ] {
                        if v[flag] == true {
                            sc[name] = json!(sc[name].as_i64().unwrap() + 1)
                        }
                    }
                }
            }
        }
    }
    Ok(Json(
        json!({"records":records,"programs":rows(&c,"SELECT data FROM school_programs ORDER BY id",[])?,"schools":schools.values().collect::<Vec<_>>()}),
    ))
}
pub(super) async fn confirm(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let mut c = lock(&s);
    let tx = c.transaction()?;
    let raw: String = tx
        .query_row(
            "SELECT data FROM school_requirements WHERE id=?",
            [&id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| ApiError::bad("用书记录不存在"))?;
    let r = parse(raw);
    if v["editionChecked"] != true {
        return Err(ApiError::bad("请核对原始书目中的版次与册次"));
    }
    let bid = if let Some(bid) = v["bookId"].as_i64() {
        book_exists(&tx, bid)?;
        bid
    } else {
        let draft = &v["book"];
        let title = required(draft, "title")?;
        let patch: crate::models::BookPatch = serde_json::from_value(
            json!({"title":title,"author":text(draft,"author"),"publisher":text(draft,"publisher"),"isbn":text(draft,"isbn"),"edition":text(draft,"edition"),"stock":0}),
        )?;
        let isbn = text(draft, "isbn");
        if !isbn.is_empty() {
            let duplicate: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM books WHERE isbn=?)",
                [isbn],
                |row| row.get(0),
            )?;
            if duplicate {
                return Err(ApiError::bad(
                    "ISBN 已有书目，请选择现有图书并核对版本，不要重复建档",
                ));
            }
        }
        let book = db::create_book(&tx, &patch)?;
        let profile = json!({"volume":text(draft,"volume"),"publicationYear":text(draft,"publicationYear"),"confirmed":true});
        tx.execute(
            "INSERT INTO book_profiles(book_id,data) VALUES(?,?)",
            params![book.id, profile.to_string()],
        )?;
        book.id
    };
    tx.execute(
        "UPDATE school_requirements SET book_id=?,state='confirmed',updated_at=? WHERE id=?",
        params![bid, db::now(), id],
    )?;
    event(
        &tx,
        "requirement",
        &id,
        "confirmed",
        &json!({"bookId":bid,"raw":r["raw"],"lookupEvidence":v["lookupEvidence"]}),
    )?;
    tx.commit()?;
    Ok(Json(json!({"bookId":bid})))
}
