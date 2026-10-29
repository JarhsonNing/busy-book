use super::*;
use axum::http::HeaderMap;
use base64::{engine::general_purpose::STANDARD, Engine};
struct Fixture {
    s: AppState,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("taoshu-agent-test-{}", rand::random::<u64>()));
        let s = AppState::new(path.clone(), 0).unwrap();
        Self { s, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let old = std::mem::replace(
            &mut *lock(&self.s),
            rusqlite::Connection::open_in_memory().unwrap(),
        );
        drop(old);
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn metadata() -> Value {
    json!({"source":"test-agent","externalId":"book-1","sourceUrl":"https://example.org/official/books","title":"数据库系统概论","author":"王珊","publisher":"高等教育出版社","isbn":"9787040406641","edition":"第5版","volume":"上册","publicationYear":"2014"})
}
#[test]
fn replay_after_book_deletion_reports_deleted_instead_of_stale_success() {
    let f=Fixture::new();
    let payload=metadata();
    let result=imports::execute(&f.s,"taoshu_import_book",&payload).unwrap();
    let id=result["bookId"].as_i64().unwrap();
    crate::book_deletion::delete(&f.s,&[id]).unwrap();
    let error=imports::execute(&f.s,"taoshu_import_book",&payload).unwrap_err();
    assert!(error.contains("已删除"));
    assert!(db::get_book(&lock(&f.s),id).unwrap().is_none());
}
#[test]
fn batch_import_keeps_successes_and_reports_each_failure_and_replay() {
    let f = Fixture::new();
    let good = metadata();
    let mut bad = good.clone();
    bad["externalId"] = json!("bad");
    bad["isbn"] = json!("9787040406642");
    let mut other = good.clone();
    other["externalId"] = json!("edition-2");
    other["edition"] = json!("第6版");
    let result = imports::batch(&f.s, &json!({"books":[good,bad,other,{"title":5}]})).unwrap();
    assert_eq!(result["created"], 2);
    assert_eq!(result["failed"], 2);
    assert_ne!(result["items"][0]["bookId"], result["items"][2]["bookId"]);
    let id = result["items"][0]["bookId"].as_i64().unwrap();
    lock(&f.s)
        .execute("UPDATE books SET stock=9 WHERE id=?", [id])
        .unwrap();
    let replay = imports::batch(&f.s, &json!({"books":[metadata()]})).unwrap();
    assert_eq!(replay["reused"], 1);
    assert_eq!(db::get_book(&lock(&f.s), id).unwrap().unwrap().stock, 9);
    let mut matched = metadata();
    matched["externalId"] = json!("second-source");
    assert_eq!(
        imports::batch(&f.s, &json!({"books":[matched]})).unwrap()["matched"],
        1
    );
    let mut changed = metadata();
    changed["title"] = json!("冲突书名");
    assert_eq!(
        imports::batch(&f.s, &json!({"books":[changed]})).unwrap()["failed"],
        1
    );
    assert!(imports::batch(&f.s, &json!({"books":[]})).is_err());
    assert!(imports::batch(&f.s, &json!({"books":vec![metadata();201]})).is_err());
    assert_eq!(
        lock(&f.s)
            .query_row("SELECT count(*) FROM books", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

const DETAIL: &str = r#"<html><meta content="数据库系统概论" property="og:title"><meta property="og:image" content="https://img.example/cover.jpg"><div id="info"><span><span class="pl">作者</span>: <a>王珊</a></span><br/><span class="pl">出版社:</span> 高等教育出版社<br><span class="pl">出版年:</span> 2014-6<br/><span class="pl">版次:</span>第5版<br><span class="pl">ISBN:</span>978-7-04-040664-1<br/></div></html>"#;
#[test]
fn isbn_parser_validates_metadata_and_never_uses_recommendation_or_captcha_isbn() {
    assert_eq!(
        isbn::detail(
            &DETAIL.replace("数据库系统概论", "O'Reilly &amp; Books"),
            "123"
        )["item"]["title"],
        "O'Reilly & Books"
    );
    let parsed = isbn::detail(DETAIL, "123");
    assert_eq!(parsed["state"], "found");
    assert_eq!(parsed["item"]["isbn"], "9787040406641");
    assert_eq!(parsed["item"]["title"], "数据库系统概论");
    assert_eq!(parsed["item"]["author"], "王珊");
    assert_eq!(parsed["item"]["publisher"], "高等教育出版社");
    assert_eq!(parsed["item"]["publicationYear"], "2014");
    assert_eq!(
        isbn::detail(&DETAIL.replace("040664-1", "040664-2"), "123")["state"],
        "isbn_missing"
    );
    let no_isbn=DETAIL.replace("978-7-04-040664-1","")+"<p>推荐 ISBN:9787040406641</p><script type='application/ld+json'>{\"@type\":\"Book\",\"name\":\"其他书\",\"isbn\":\"9787040406641\"}</script>";
    assert_eq!(isbn::detail(&no_isbn, "123")["state"], "isbn_missing");
    let structured=no_isbn+"<script type='application/ld+json'>{\"@type\":\"Book\",\"name\":\"数据库系统概论\",\"isbn\":\"9787040406641\"}</script>";
    assert_eq!(
        isbn::detail(&structured, "123")["item"]["isbn"],
        "9787040406641"
    );
    assert_eq!(
        isbn::detail("<title>安全验证</title><p>请输入验证码</p>", "123")["state"],
        "blocked"
    );
}

struct MockDouban {
    calls: Vec<String>,
    blocked: bool,
}
impl isbn::Fetcher for MockDouban {
    fn get(&mut self, url: &str) -> Value {
        self.calls.push(url.into());
        if self.blocked {
            return json!({"state":"blocked","error":"模拟限流","retryAfterSeconds":60});
        }
        let body = if url.contains("subject_suggest") {
            json!([{"id":"123","title":"数据库系统概论","author_name":"王珊","pic":"https://img.example/cover.jpg"},{"id":"456","title":"数据库系统概论","author_name":"王珊"},{"id":"../../evil","title":"假候选"}]).to_string()
        } else {
            DETAIL.into()
        };
        json!({"state":"ok","body":body})
    }
}
#[test]
fn isbn_lookup_caches_across_calls_retains_candidates_and_handles_blocked_provider() {
    let f = Fixture::new();
    let mut remote = MockDouban {
        calls: vec![],
        blocked: false,
    };
    let result = isbn::lookup_with(&f.s, &json!({"query":"数据库系统概论"}), &mut remote).unwrap();
    assert_eq!(remote.calls.len(), 2);
    assert!(remote.calls[0].starts_with("https://book.douban.com/j/subject_suggest?q="));
    assert_eq!(remote.calls[1], "https://book.douban.com/subject/123/");
    assert_eq!(result["selectionRequired"], true);
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["items"][0]["isbn"], "9787040406641");
    assert!(result["items"][1]["isbn"].is_null());
    assert_eq!(
        isbn::lookup_with(&f.s, &json!({"query":"数据库系统概论"}), &mut remote).unwrap()["cached"],
        true
    );
    assert_eq!(remote.calls.len(), 2);
    assert_eq!(
        isbn::lookup_with(&f.s, &json!({"subjectId":"123"}), &mut remote).unwrap()["cached"],
        true
    );
    assert_eq!(remote.calls.len(), 2);
    lock(&f.s)
        .execute(
            "UPDATE agent_isbn_cache SET expires_at=0 WHERE cache_key='subject:123'",
            [],
        )
        .unwrap();
    remote.blocked = true;
    assert_eq!(
        isbn::lookup_with(&f.s, &json!({"subjectId":"123"}), &mut remote).unwrap()["state"],
        "blocked"
    );
    assert_eq!(remote.calls.len(), 3);
    assert_eq!(
        isbn::lookup_with(&f.s, &json!({"subjectId":"123"}), &mut remote).unwrap()["cached"],
        true
    );
    assert_eq!(remote.calls.len(), 3);
    assert!(isbn::lookup_with(
        &f.s,
        &json!({"subjectId":"https://evil.example/"}),
        &mut remote
    )
    .is_err());
    assert_eq!(remote.calls.len(), 3);
    assert_eq!(
        lock(&f.s)
            .query_row("SELECT count(*) FROM books", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn exported_config_preserves_user_fields_and_keeps_the_app_token_private() {
    let f = Fixture::new();
    let cfg = prepare(&f.s).unwrap();
    let workflow = PathBuf::from(cfg["workflowSkillPath"].as_str().unwrap());
    assert_eq!(fs::read_to_string(&workflow).unwrap(), WORKFLOW_SKILL);
    assert_eq!(
        workflow.parent().unwrap().parent().unwrap(),
        PathBuf::from(cfg["skillPath"].as_str().unwrap())
            .parent()
            .unwrap()
            .parent()
            .unwrap()
    );
    let project = PathBuf::from(cfg["workspace"].as_str().unwrap()).join("opencode.json");
    let mut custom: Value = serde_json::from_slice(&fs::read(&project).unwrap()).unwrap();
    custom["model"] = json!("local/example");
    custom["mcp"]["servers"]["other"] = json!({"type":"local","command":["example"]});
    fs::write(&project, serde_json::to_vec(&custom).unwrap()).unwrap();
    prepare(&f.s).unwrap();
    prepare(&f.s).unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(&project).unwrap()).unwrap();
    assert_eq!(saved["model"], "local/example");
    assert_eq!(
        saved["mcp"]["servers"]["other"],
        custom["mcp"]["servers"]["other"]
    );
    let connection: Value =
        serde_json::from_slice(&fs::read(support(&f.s).join("connection.json")).unwrap()).unwrap();
    assert_ne!(connection["token"], f.s.token);
    assert!(!cfg
        .to_string()
        .contains(connection["token"].as_str().unwrap()));
    assert!(!cfg.to_string().contains(&f.s.token));
    let qoder=PathBuf::from(cfg["workspace"].as_str().unwrap()).join(".mcp.json");
    let mut custom:Value=serde_json::from_slice(&fs::read(&qoder).unwrap()).unwrap();
    custom["mcpServers"]["other"]=json!({"command":"example"});
    fs::write(&qoder,custom.to_string()).unwrap();
    prepare(&f.s).unwrap();
    let saved:Value=serde_json::from_slice(&fs::read(&qoder).unwrap()).unwrap();
    assert_eq!(saved["mcpServers"]["other"],custom["mcpServers"]["other"]);
    assert_eq!(saved["mcpServers"]["taoshu"],cfg["qoder"]["mcpServers"]["taoshu"]);
    assert_eq!(fs::read_to_string(cfg["qoderSkillPath"].as_str().unwrap()).unwrap(),SKILL);
    assert_eq!(fs::read_to_string(cfg["qoderWorkflowSkillPath"].as_str().unwrap()).unwrap(),WORKFLOW_SKILL);
    fs::write(&qoder,b"invalid JSON").unwrap();
    assert!(prepare(&f.s).is_err());assert_eq!(fs::read(&qoder).unwrap(),b"invalid JSON");
    fs::write(&qoder,custom.to_string()).unwrap();
    fs::write(&project, b"invalid JSON").unwrap();
    assert!(prepare(&f.s).is_err());
    assert_eq!(fs::read(&project).unwrap(), b"invalid JSON");
}
#[tokio::test]
async fn interrupted_download_resumes_the_verified_range() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/archive", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 512];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
            }
            if attempt == 0 {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc",
                    )
                    .unwrap();
            } else {
                assert!(String::from_utf8_lossy(&request)
                    .to_lowercase()
                    .contains("range: bytes=3-"));
                stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef").unwrap();
            }
        }
    });
    let f = Fixture::new();
    let archive = f.path.join("resume.tgz");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    assert!(opencode::download(&client, &f.s, &url, &archive)
        .await
        .is_err());
    assert_eq!(fs::read(&archive).unwrap(), b"abc");
    opencode::download(&client, &f.s, &url, &archive)
        .await
        .unwrap();
    assert_eq!(fs::read(&archive).unwrap(), b"abcdef");
    server.join().unwrap();
}
#[test]
fn imports_are_idempotent_version_scoped_and_preserve_evidence() {
    let f = Fixture::new();
    let v = metadata();
    let first = imports::execute(&f.s, "taoshu_import_book", &v).unwrap();
    let id = first["bookId"].as_i64().unwrap();
    assert_eq!(
        imports::execute(&f.s, "taoshu_import_book", &v).unwrap()["reused"],
        true
    );
    let mut changed = v.clone();
    changed["title"] = json!("另一本书");
    assert!(imports::execute(&f.s, "taoshu_import_book", &changed).is_err());
    changed = v.clone();
    changed["externalId"] = json!("book-2");
    changed["edition"] = json!("第6版");
    assert_ne!(
        imports::execute(&f.s, "taoshu_import_book", &changed).unwrap()["bookId"],
        id
    );
    changed = v.clone();
    changed["externalId"] = json!("book-3");
    changed["volume"] = json!("下册");
    assert_ne!(
        imports::execute(&f.s, "taoshu_import_book", &changed).unwrap()["bookId"],
        id
    );
    let c = lock(&f.s);
    let data: String = c
        .query_row(
            "SELECT data FROM book_profiles WHERE book_id=?",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let p: Value = serde_json::from_str(&data).unwrap();
    assert_eq!(p["agentEvidence"][0]["url"], v["sourceUrl"]);
    let b = db::get_book(&c, id).unwrap().unwrap();
    assert_eq!(b.stock, 0);
    assert!(b.cost_price.is_none());
}
#[test]
fn school_and_sources_import_to_the_same_book_without_overwriting_reviewed_quotes() {
    let f = Fixture::new();
    let id = imports::execute(&f.s, "taoshu_import_book", &metadata()).unwrap()["bookId"]
        .as_i64()
        .unwrap();
    let mut school = json!({"source":"agent","externalId":"citation","sourceUrl":"https://example.org/catalog","bookId":id,"school":"测试大学","year":2027,"major":"计算机","raw":"数据库系统概论（第5版）上册","edition":"第5版","volume":"下册"});
    assert!(imports::execute(&f.s, "taoshu_import_school_book", &school).is_err());
    school["volume"] = json!("上册");
    let r = imports::execute(&f.s, "taoshu_import_school_book", &school).unwrap();
    assert!(r["requirementId"].is_string());
    let mut source = json!({"source":"agent","externalId":"offer-1","sourceUrl":"https://example.org/catalog","bookId":id,"url":"https://item.taobao.com/item.htm?id=123","price":12.5,"freight":null});
    let a = imports::execute(&f.s, "taoshu_import_source", &source).unwrap();
    let sid = a["sourceId"].as_i64().unwrap();
    lock(&f.s).execute("UPDATE book_sources SET data=json_set(data,'$.price',20,'$.status','available') WHERE id=?",[sid]).unwrap();
    source["externalId"] = json!("offer-2");
    source["price"] = json!(1);
    assert_eq!(
        imports::execute(&f.s, "taoshu_import_source", &source).unwrap()["sourceId"],
        sid
    );
    let raw: String = lock(&f.s)
        .query_row("SELECT data FROM book_sources WHERE id=?", [sid], |r| {
            r.get(0)
        })
        .unwrap();
    let data: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(data["price"], 20);
    assert_eq!(data["status"], "available");
}
#[test]
fn images_are_actual_validated_uploads_and_replay_does_not_duplicate() {
    let f = Fixture::new();
    let id = imports::execute(&f.s, "taoshu_import_book", &metadata()).unwrap()["bookId"]
        .as_i64()
        .unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let v = json!({"source":"agent","externalId":"cover","sourceUrl":"https://example.org/cover.png","bookId":id,"base64":STANDARD.encode(bytes.into_inner()),"mimeType":"image/png"});
    let first = imports::execute(&f.s, "taoshu_import_image", &v).unwrap();
    assert!(first["imageId"].is_number());
    assert_eq!(
        imports::execute(&f.s, "taoshu_import_image", &v).unwrap()["reused"],
        true
    );
    let mut invalid = v;
    invalid["externalId"] = json!("bad-image");
    invalid["base64"] = json!(STANDARD.encode(b"<html>not an image</html>"));
    assert!(imports::execute(&f.s, "taoshu_import_image", &invalid).is_err());
    assert_eq!(
        lock(&f.s)
            .query_row("SELECT COUNT(*) FROM images WHERE book_id=?", [id], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        1
    );
}
#[test]
fn rpc_catalog_has_no_transaction_or_secret_tools_and_checks_arguments() {
    let f = Fixture::new();
    let catalog = mcp::dispatch(&f.s, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    let t = catalog["result"]["tools"].as_array().unwrap();
    assert_eq!(t.len(), 9);
    for tool in t {
        let name = tool["name"].as_str().unwrap();
        assert!(!name.contains("ship") && !name.contains("order") && !name.contains("secret"));
    }
    let rejected = mcp::dispatch(
        &f.s,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"taoshu_import_book","arguments":{"stock":99}}}),
    );
    assert_eq!(rejected["error"]["code"], -32602);
    let rejected = mcp::dispatch(
        &f.s,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"taoshu_search_books","arguments":{"query":1}}}),
    );
    assert_eq!(rejected["error"]["code"], -32602);
    assert!(mcp::dispatch(&f.s,json!({"jsonrpc":"2.0","id":4,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}))["result"]["capabilities"]["tools"].is_object());
}
#[test]
fn token_scope_and_official_release_checks_are_enforced() {
    let f = Fixture::new();
    let token = db::get_setting(&lock(&f.s), "agentMcpToken")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    assert!(!mcp::authorized(&f.s, &headers));
    db::set_setting(&lock(&f.s), "agentMcpEnabled", &json!(true)).unwrap();
    assert!(mcp::authorized(&f.s, &headers));
    headers.insert("origin", "https://evil.example".parse().unwrap());
    assert!(!mcp::origin_allowed(&headers));
    assert!(!mcp::authorized(&f.s, &headers));
    let name = "@opencode/cli-windows-x64-baseline";
    let mut release = json!({"name":name,"version":opencode::VERSION,"dist":{"tarball":format!("https://registry.npmjs.org/{name}/-/cli-windows-x64-baseline-{}.tgz",opencode::VERSION),"integrity":format!("sha512-{}",STANDARD.encode([0u8;64]))}});
    assert!(opencode::package_asset(&release, name).is_ok());
    release["dist"]["tarball"] = json!("https://evil.example/opencode.tgz");
    assert!(opencode::package_asset(&release, name).is_err());
}
#[tokio::test]
async fn http_mcp_auth_notifications_and_revocation() {
    let f = Fixture::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = crate::api::router(f.s.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::new();
    let token = db::get_setting(&lock(&f.s), "agentMcpToken")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}});
    assert_eq!(
        client
            .post(format!("{base}/mcp"))
            .bearer_auth(&token)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    db::set_setting(&lock(&f.s), "agentMcpEnabled", &json!(true)).unwrap();
    assert!(client
        .post(format!("{base}/mcp"))
        .bearer_auth(&token)
        .json(&request)
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    assert_eq!(
        client
            .post(format!("{base}/mcp"))
            .bearer_auth(&token)
            .header("origin", "https://evil.example")
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{base}/api/settings"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .post(format!("{base}/mcp"))
            .header("x-kf-token", &f.s.token)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .post(format!("{base}/mcp"))
            .bearer_auth(&token)
            .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .unwrap()
            .status(),
        202
    );
    assert_eq!(
        client
            .get(format!("{base}/mcp"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    db::set_setting(&lock(&f.s), "agentMcpEnabled", &json!(false)).unwrap();
    assert_eq!(
        client
            .post(format!("{base}/mcp"))
            .bearer_auth(&token)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    server.abort();
}
