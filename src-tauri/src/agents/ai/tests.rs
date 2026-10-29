use super::*;
use crate::models::{BookPatch, ListingInput};
fn conn() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    db::init(&c).unwrap();
    c
}
fn book(c: &Connection) -> i64 {
    db::create_book(c,&serde_json::from_value::<BookPatch>(json!({"title":"测试图书","isbn":"9787040406641","edition":"第5版","costPrice":8,"sellPrice":20,"stock":0})).unwrap()).unwrap().id
}
fn source(c: &Connection, id: i64, price: f64, freight: Option<f64>, primary: bool) -> i64 {
    let quote: i64 = c
        .query_row(
            "SELECT coalesce(max(id),0)+1001 FROM book_sources",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let d = json!({"url":format!("https://item.taobao.com/item.htm?id={quote}"),"title":"测试货源","price":price,"freight":freight,"status":"available","dropship":true,"sku":"第5版","checkedAt":"2026-10-29"});
    c.execute(
        "INSERT INTO book_sources(book_id,is_primary,data,updated_at) VALUES(?,?,?,?)",
        params![id, primary, d.to_string(), db::now()],
    )
    .unwrap();
    c.last_insert_rowid()
}
fn input() -> ListingInput {
    ListingInput {
        title: None,
        description: None,
        price: None,
        condition: None,
    }
}
#[test]
fn engine_models_migrate_and_requests_keep_their_original_executor() {
    let c = conn();
    db::set_setting(&c,"agentAi",&json!({"model":"old/provider-model"})).unwrap();
    let migrated=config(&c);
    assert_eq!(migrated["provider"],"opencode");
    assert_eq!(migrated["models"]["opencode"],"old/provider-model");
    assert_eq!(migrated["models"]["qoder"],"");
    let mut cfg=defaults();cfg["provider"]=json!("qoder");cfg["model"]=json!("Performance");cfg["models"]["opencode"]=json!("fixture/writer");
    db::set_setting(&c,"agentAi",&cfg).unwrap();
    let frozen=request(&c,&json!({"task":"text"})).unwrap();
    assert_eq!(frozen["provider"],"qoder");assert_eq!(frozen["model"],"Performance");
    assert_eq!(request(&c,&json!({"provider":"opencode"})).unwrap()["model"],"fixture/writer");
    db::set_setting(&c,"agentAi",&defaults()).unwrap();
    assert_eq!(frozen["provider"],"qoder");assert_eq!(frozen["model"],"Performance");
    assert!(request(&c,&json!({"provider":"unknown"})).is_err());
    assert!(model("auto","qoder").is_ok());assert!(model("Auto Model","qoder").is_ok());assert!(model("auto","opencode").is_err());
    assert!(model("auto & echo secret","qoder").is_err());
}
#[test]
fn qoder_results_require_success_and_never_expose_provider_details() {
    let success=json!({"type":"result","subtype":"success","is_error":false,"result":"```json\n{\"text\":\"Qoder 草稿\"}\n```"});
    assert_eq!(parse_qoder_output(&success.to_string()).unwrap()["text"],"Qoder 草稿");
    let partial=json!({"type":"result","subtype":"error_max_turns","result":"{\"text\":\"不应写入\"}"});
    assert!(parse_qoder_output(&partial.to_string()).is_err());
    let error=json!({"type":"result","subtype":"error_during_execution","is_error":true,"errors":["Not logged in. private-token-secret"]});
    let message=parse_qoder_output(&error.to_string()).unwrap_err();assert!(message.contains("登录"));assert!(!message.contains("secret"));
    assert!(parse_qoder_output(r#"{"type":"assistant","result":"partial"}"#).is_err());
}
#[test]
fn queued_jobs_are_deduplicated_within_each_executor() {
    let path=std::env::temp_dir().join(format!("taoshu-executors-{}",rand::random::<u64>()));
    let s=AppState::new(path.clone(),0).unwrap();
    let program=path.join("fixture.exe");fs::write(&program,[]).unwrap();
    let id={let c=lock(&s);for key in ["agentOpenCode","agentQoder"] {db::set_setting(&c,key,&json!({"mode":"local","path":program})).unwrap();}book(&c)};
    let first=enqueue(&s,&json!({"bookIds":[id],"provider":"opencode"})).unwrap();
    let other=enqueue(&s,&json!({"bookIds":[id],"provider":"qoder"})).unwrap();
    let replay=enqueue(&s,&json!({"bookIds":[id],"provider":"qoder"})).unwrap();
    assert_ne!(first["id"],other["id"]);assert_eq!(other["id"],replay["id"]);
    drop(s);assert!(path.starts_with(std::env::temp_dir()));fs::remove_dir_all(path).unwrap();
}
#[test]
fn cost_uses_second_lowest_product_price_separate_from_freight_and_primary_selection() {
    let c = conn();
    let id = book(&c);
    source(&c, id, 1.0, None, false);
    let verified = source(&c, id, 10.0, Some(2.0), false);
    let ctx = context(&c, id).unwrap();
    let basis = cost_basis(&ctx, None);
    assert_eq!(basis["sourceId"], verified);
    assert_eq!(basis["cost"], 10.0);
    assert_eq!(basis["freight"], 2.0);
    assert_eq!(basis["landedCost"], 12.0);
    c.execute(
        "UPDATE book_sources SET is_primary=1 WHERE id=(SELECT min(id) FROM book_sources)",
        [],
    )
    .unwrap();
    let basis = cost_basis(&context(&c, id).unwrap(), None);
    assert_eq!(basis["cost"], 10.0);
}
#[test]
fn reference_cost_requires_two_distinct_valid_taobao_prices() {
    let c = conn();
    let id = book(&c);
    source(&c, id, 2.0, Some(0.0), false);
    source(&c, id, 2.0, None, false);
    assert!(cost_basis(&context(&c, id).unwrap(), None)["cost"].is_null());
    let next = source(&c, id, 3.0, None, false);
    c.execute("UPDATE book_sources SET data=json_set(data,'$.status','pending','$.dropship',0) WHERE id=?",[next]).unwrap();
    let basis = cost_basis(&context(&c, id).unwrap(), None);
    assert_eq!(basis["cost"], 3.0);
    assert!(basis["freight"].is_null());
    assert_eq!(basis["verified"], false);
    c.execute(
        "UPDATE book_sources SET data=json_set(data,'$.url','https://example.org/book') WHERE id=?",
        [next],
    )
    .unwrap();
    assert!(cost_basis(&context(&c, id).unwrap(), None)["cost"].is_null());
}
#[test]
fn ai_saves_draft_and_enforces_profit_floor_without_selecting_source() {
    let mut c = conn();
    let id = book(&c);
    source(&c, id, 1.0, None, false);
    source(&c, id, 10.0, Some(2.0), false);
    let ctx = context(&c, id).unwrap();
    let expected = fingerprint(&c, id).unwrap();
    let tx = c.transaction().unwrap();
    let result=apply(&tx,id,10,&json!({"task":"complete","model":"test/model"}),&ctx,&expected,&json!({"title":"测试上架标题","description":"已核对版本的图书描述","sellPrice":1,"analysis":"比较了货源到手成本"})).unwrap();
    tx.commit().unwrap();
    assert_eq!(result["costPrice"], 10.0);
    assert_eq!(result["sellPrice"], 17.0);
    let b = db::get_book(&c, id).unwrap().unwrap();
    assert_eq!(b.cost_price, Some(10.0));
    assert_eq!(b.sell_price, Some(17.0));
    assert_eq!(b.stock, 0);
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM book_sources WHERE is_primary=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let copy = listing_defaults(&c, id, input()).unwrap();
    assert_eq!(copy.title.as_deref(), Some("测试上架标题"));
    assert_eq!(copy.price, Some(17.0));
    let explicit = listing_defaults(
        &c,
        id,
        ListingInput {
            title: Some("人工确认标题".into()),
            price: Some(21.0),
            ..input()
        },
    )
    .unwrap();
    assert_eq!(explicit.price, Some(21.0));
    assert_eq!(explicit.title.as_deref(), Some("人工确认标题"));
}
#[test]
fn unknown_cost_preserves_prices_and_invalid_copy_rolls_back_price_changes() {
    let mut c = conn();
    let id = book(&c);
    source(&c, id, 10.0, None, true);
    let ctx = context(&c, id).unwrap();
    let expected = fingerprint(&c, id).unwrap();
    let tx = c.transaction().unwrap();
    let d = apply(
        &tx,
        id,
        1,
        &json!({"task":"pricing"}),
        &ctx,
        &expected,
        &json!({"sellPrice":999,"analysis":"运费待核实"}),
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(d["costPrice"].is_null());
    assert!(d["sellPrice"].is_null());
    assert_eq!(
        db::get_book(&c, id).unwrap().unwrap().sell_price,
        Some(20.0)
    );
    c.execute(
        "UPDATE book_sources SET data=json_set(data,'$.freight',0)",
        [],
    )
    .unwrap();
    source(&c, id, 5.0, Some(0.0), false);
    let ctx = context(&c, id).unwrap();
    let expected = fingerprint(&c, id).unwrap();
    {
        let tx = c.transaction().unwrap();
        assert!(apply(&tx,id,2,&json!({"task":"complete"}),&ctx,&expected,&json!({"title":"x".repeat(31),"description":"test","analysis":"valid analysis","sellPrice":99})).is_err());
    }
    assert_eq!(
        db::get_book(&c, id).unwrap().unwrap().sell_price,
        Some(20.0)
    );
}
#[test]
fn concurrent_edit_is_preserved_and_stale_copy_is_not_reused_by_text_generation() {
    let mut c = conn();
    let id = book(&c);
    source(&c, id, 10.0, Some(0.0), false);
    let ctx = context(&c, id).unwrap();
    let expected = fingerprint(&c, id).unwrap();
    c.execute("UPDATE books SET sell_price=23 WHERE id=?", [id])
        .unwrap();
    assert!(apply(
        &c,
        id,
        1,
        &json!({"task":"pricing"}),
        &ctx,
        &expected,
        &json!({"sellPrice":99,"analysis":"test"})
    )
    .unwrap_err()
    .contains("修改"));
    let expected = fingerprint(&c, id).unwrap();
    let tx = c.transaction().unwrap();
    apply(
        &tx,
        id,
        2,
        &json!({"task":"copy"}),
        &ctx,
        &expected,
        &json!({"title":"旧版草稿","description":"旧版描述"}),
    )
    .unwrap();
    tx.commit().unwrap();
    c.execute("UPDATE books SET edition='第6版' WHERE id=?", [id])
        .unwrap();
    assert!(listing_defaults(&c, id, input()).unwrap().title.is_none());
    let ctx = context(&c, id).unwrap();
    let expected = fingerprint(&c, id).unwrap();
    let tx = c.transaction().unwrap();
    apply(
        &tx,
        id,
        3,
        &json!({"task":"text"}),
        &ctx,
        &expected,
        &json!({"text":"新版自定义文本"}),
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(listing_defaults(&c, id, input()).unwrap().title.is_none());
    assert_eq!(draft(&c, id).unwrap()["draft"]["stale"], true);
}
#[test]
fn parses_cli_text_events_and_reports_quota_without_returning_provider_secrets() {
    let event =
        json!({"type":"text","part":{"text":"```json\n{\"text\":\"测试\"}\n```"}}).to_string();
    assert_eq!(parse_output(&event).unwrap()["text"], "测试");
    assert!(parse_output(&json!({"type":"text","text":"invalid"}).to_string()).is_err());
    let raw = r#"{"type":"error","error":{"type":"provider.free-limit","status":429,"response":{"body":"private-provider-detail"}}}"#;
    let error = provider_error(raw, "apikey=secret");
    assert!(error.contains("429"));
    assert!(!error.contains("secret"));
    assert!(parse_output(raw).is_err());
    assert!(model("--execute evil", "opencode").is_err());
    assert!(request(&conn(), &json!({"task":"shell"})).is_err());
}
#[test]
fn latest_search_prices_win_over_old_source_prices_and_missing_rank_clears_stale_cost() {
    let c = conn();
    let id = book(&c);
    source(&c, id, 50.0, Some(0.0), false);
    let data = json!({"items":[{"url":"https://item.taobao.com/item.htm?id=1001","price":8},{"url":"https://item.taobao.com/item.htm?id=1002","price":3},{"url":"https://item.taobao.com/item.htm?id=1003","price":8},{"url":"https://item.taobao.com/item.htm?id=1004","price":12},{"url":"https://example.org/not-taobao","price":0.1},{"url":"https://item.taobao.com/item.htm?id=1005","price":0}]});
    c.execute(
        "INSERT INTO price_searches(book_id,data) VALUES(?,?)",
        params![id, data.to_string()],
    )
    .unwrap();
    let basis = cost_basis(&context(&c, id).unwrap(), Some(999));
    assert_eq!(basis["cost"], 8.0);
    assert_eq!(basis["distinctPrices"], 3);
    assert!(basis["freight"].is_null());
    c.execute("UPDATE books SET cost_price=99 WHERE id=?", [id])
        .unwrap();
    refresh_reference_cost(&c, id).unwrap();
    assert_eq!(db::get_book(&c, id).unwrap().unwrap().cost_price, Some(8.0));
    db::update_book(
        &c,
        id,
        &serde_json::from_value::<BookPatch>(json!({"costPrice":999})).unwrap(),
    )
    .unwrap();
    assert_eq!(db::get_book(&c, id).unwrap().unwrap().cost_price, Some(8.0));
    c.execute(
        "UPDATE price_searches SET data=? WHERE book_id=?",
        params![
            json!({"items":[{"url":"https://item.taobao.com/item.htm?id=1001","price":8}]})
                .to_string(),
            id
        ],
    )
    .unwrap();
    refresh_reference_cost(&c, id).unwrap();
    assert_eq!(db::get_book(&c, id).unwrap().unwrap().cost_price, None);
}
#[test]
fn automatic_hooks_are_opt_in_coalesced_and_ignore_price_only_edits() {
    let c = conn();
    let first = book(&c);
    assert_eq!(
        c.query_row("SELECT count(*) FROM collection_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let mut cfg = defaults();
    cfg["enabled"] = json!(true);
    db::set_setting(&c, "agentAi", &cfg).unwrap();
    auto_queue(&c, first, "book").unwrap();
    auto_queue(&c, first, "source").unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM collection_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    c.execute("UPDATE collection_jobs SET state='completed'", [])
        .unwrap();
    db::update_book(
        &c,
        first,
        &serde_json::from_value::<BookPatch>(json!({"title":"测试图书","sellPrice":25})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM collection_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    db::update_book(
        &c,
        first,
        &serde_json::from_value::<BookPatch>(json!({"edition":"第6版"})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM collection_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}
