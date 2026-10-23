use super::*;
use crate::models::{BookPatch, CaptureInput};
use std::{fs, path::PathBuf};

struct Fixture {
    state: AppState,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "taoshu-commerce-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let state = AppState::new(path.clone(), 0).unwrap();
        db::set_setting(&lock(&state), "xianyuAppKey", &json!("test-app")).unwrap();
        Self { state, path }
    }
    fn book(&self, title: &str) -> i64 {
        db::create_book(
            &lock(&self.state),
            &serde_json::from_value::<BookPatch>(
                json!({"title":title,"isbn":"9787300000001","edition":"第2版","stock":7}),
            )
            .unwrap(),
        )
        .unwrap()
        .id
    }
    fn channel(&self, bid: i64, mode: &str) {
        lock(&self.state).execute("INSERT INTO channel_products(book_id,app_key,shop,product_id,data,updated_at) VALUES(?,'test-app','shop','9007199254740997',?,'now')",params![bid,json!({"mode":mode,"title":"教材","itemId":"8007199254740997","physicalSkuId":"9007199254741001","digitalSkuId":"9007199254741002","state":"published"}).to_string()]).unwrap();
    }
    fn observe(&self, order: &Value) {
        observe_order(&lock(&self.state), "test-app", order).unwrap();
    }
    fn orders(&self) -> Vec<Value> {
        fulfillment::all(&lock(&self.state)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the unique test directory is cleaned; SQLite handles may still be held on Windows.
        let _ = lock(&self.state).execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
        let old = std::mem::replace(
            &mut *lock(&self.state),
            Connection::open_in_memory().unwrap(),
        );
        drop(old);
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn order(no: &str, sku: &str) -> Value {
    json!({"order_no":no,"seller_name":"shop","order_status":12,"refund_status":0,"refund_amount":0,"refund_time":0,"pay_amount":5000,"update_time":100,"goods":{"title":"教材","quantity":1,"product_id":9007199254740997u64,"item_id":8007199254740997u64,"sku_id":sku}})
}
fn catalog() -> Value {
    json!({"schools":[{"id":"s","name":"甲大学"},{"id":"s2","name":"乙大学"}],"sources":[{"id":"source","collectedAt":"2026-10-20"}],"programs":[{"id":"p","schoolId":"s","sourceId":"source","year":2027,"college":"数学学院","majorName":"数学","majorCode":"0701"},{"id":"p2","schoolId":"s2","sourceId":"source","year":2027,"majorName":"数学"}],"subjects":[{"id":"sub","programId":"p","stage":"初试","name":"数学分析"},{"id":"sub2","programId":"p2","stage":"初试","name":"数学分析"}],"books":[{"id":"b","title":"数学分析","editions":["第2版","第3版"],"volumes":["上册","下册"]}],"requirements":[{"id":"r","subjectId":"sub","textbookId":"b","raw":"数学分析（第2版）上册","sourceUrl":"https://example.org/catalog"},{"id":"r2","subjectId":"sub2","textbookId":"b","raw":"数学分析（第2版）上册"}]})
}
async fn source(f: &Fixture, bid: i64, primary: bool, price: f64) -> i64 {
    save_source(State(f.state.clone()),Json(json!({"bookId":bid,"primary":primary,"data":{"url":"https://item.taobao.com/item.htm?id=123","shop":"供货店","sku":"第2版 上册","status":"available","dropship":true,"price":price,"freight":2,"checkedAt":"2026-10-23T01:00:00Z"}}))).await.unwrap().0["id"].as_i64().unwrap()
}

#[tokio::test]
async fn school_preview_repeat_and_multi_school_confirmation() {
    let f = Fixture::new();
    let bid = f.book("数学分析");
    let input = catalog();
    let p = catalog::preview(State(f.state.clone()), Json(input.clone()))
        .await
        .unwrap()
        .0;
    assert_eq!(p["count"], 2);
    assert_eq!(p["records"][0]["ambiguous"], true);
    assert!(
        catalog::list(State(f.state.clone())).await.unwrap().0["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let _ = catalog::import(State(f.state.clone()), Json(input.clone()))
        .await
        .unwrap();
    for r in p["records"].as_array().unwrap() {
        let _ = catalog::confirm(
            State(f.state.clone()),
            Path(text(r, "id").into()),
            Json(json!({"bookId":bid,"editionChecked":true})),
        )
        .await
        .unwrap();
    }
    let mut newer = input.clone();
    newer["sources"][0]["collectedAt"] = json!("2026-10-23");
    assert_eq!(
        catalog::import(State(f.state.clone()), Json(newer))
            .await
            .unwrap()
            .0["unchanged"],
        2
    );
    let result = catalog::list(State(f.state.clone())).await.unwrap().0;
    assert!(result["records"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["bookId"] == bid && r["state"] == "confirmed"));
    assert_eq!(
        books(State(f.state.clone()))
            .await
            .unwrap()
            .0
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut changed = input;
    changed["requirements"][0]["raw"] = json!("数学分析第3版 下册");
    assert_eq!(
        catalog::import(State(f.state.clone()), Json(changed))
            .await
            .unwrap()
            .0["changed"],
        1
    );
    assert_eq!(
        catalog::list(State(f.state.clone())).await.unwrap().0["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["state"] == "pending")
            .count(),
        1
    );
}
#[tokio::test]
async fn different_editions_are_not_automatically_merged() {
    let f = Fixture::new();
    let input = catalog();
    let records = catalog::flatten(&input).unwrap();
    let _ = catalog::import(State(f.state.clone()), Json(input))
        .await
        .unwrap();
    for (i, r) in records.iter().enumerate() {
        let _ = catalog::confirm(State(f.state.clone()),Path(text(r,"id").into()),Json(json!({"editionChecked":true,"book":{"title":"数学分析","edition":format!("第{}版",i+2),"volume":"上册"}}))).await.unwrap();
    }
    assert_eq!(
        books(State(f.state.clone()))
            .await
            .unwrap()
            .0
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(catalog::confirm(
        State(f.state.clone()),
        Path(text(&records[0], "id").into()),
        Json(json!({"bookId":1}))
    )
    .await
    .is_err());
}
#[tokio::test]
async fn candidates_primary_sources_and_immutable_resource_versions() {
    let f = Fixture::new();
    let input:CaptureInput=serde_json::from_value(json!({"itemId":"123","url":"https://item.taobao.com/item.htm?id=123","title":"候选","asSource":true})).unwrap();
    let (capture, book, _) =
        db::upsert_capture(&lock(&f.state), &input, 0.3, 5.0, "九成新").unwrap();
    assert!(book.is_none());
    capture_candidate(&lock(&f.state), &capture).unwrap();
    capture_candidate(&lock(&f.state), &capture).unwrap();
    assert_eq!(
        books(State(f.state.clone()))
            .await
            .unwrap()
            .0
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(source_list(&lock(&f.state), None).unwrap().len(), 1);
    let bid = f.book("数学分析");
    source(&f, bid, true, 10.0).await;
    source(&f, bid, true, 12.0).await;
    assert_eq!(
        source_list(&lock(&f.state), Some(bid))
            .unwrap()
            .iter()
            .filter(|r| r["primary"] == true)
            .count(),
        1
    );
    let payload = json!({"version":"v1","url":"https://example.org/drive","instructions":"打开链接下载","code":"abcd"});
    let id = add_resource(State(f.state.clone()), Path(bid), Json(payload.clone()))
        .await
        .unwrap()
        .0["id"]
        .as_i64()
        .unwrap();
    assert!(
        add_resource(State(f.state.clone()), Path(bid), Json(payload))
            .await
            .is_err()
    );
    assert!(resource_state(
        State(f.state.clone()),
        Path(id),
        Json(json!({"state":"ready"}))
    )
    .await
    .is_err());
    let _ = resource_state(
        State(f.state.clone()),
        Path(id),
        Json(json!({"state":"ready","checked":true})),
    )
    .await
    .unwrap();
    let c = lock(&f.state);
    assert_eq!(
        summary(&c, &db::get_book(&c, bid).unwrap().unwrap()).unwrap()["digitalReady"],
        true
    );
}
#[test]
fn exact_mapping_app_shop_spec_and_string_identifiers() {
    let f = Fixture::new();
    let bid = f.book("数学分析");
    f.channel(bid, "mixed");
    let mut o = order("1", "9007199254741001");
    f.observe(&o);
    o["order_no"] = json!("2");
    o["seller_name"] = json!("wrong-shop");
    o["goods"]["item_id"] = json!("wrong-item");
    f.observe(&o);
    o = order("3", "");
    f.observe(&o);
    o = order("4", "0");
    f.observe(&o);
    observe_order(
        &lock(&f.state),
        "another-app",
        &order("5", "9007199254741001"),
    )
    .unwrap();
    let all = f.orders();
    assert_eq!(all.len(), 4);
    assert_eq!(all.iter().filter(|o| o["kind"] == "physical").count(), 1);
    let known = all.iter().find(|o| o["orderNo"] == "1").unwrap();
    assert_eq!(known["snapshot"]["goods"]["product_id"], "9007199254740997");
    assert!(all
        .iter()
        .filter(|o| o["kind"] == "unknown")
        .all(|o| o["bookId"].is_null()));
}
#[tokio::test]
async fn physical_repeat_sync_purchase_replacement_and_refund() {
    let f = Fixture::new();
    let bid = f.book("数学分析");
    let first = source(&f, bid, true, 10.0).await;
    f.channel(bid, "mixed");
    let mut o = order("physical", "9007199254741001");
    f.observe(&o);
    f.observe(&o);
    assert_eq!(f.orders().len(), 1);
    let id = f.orders()[0]["id"].as_i64().unwrap();
    let backup = source(&f, bid, true, 20.0).await;
    f.observe(&o);
    assert_eq!(f.orders()[0]["data"]["source"]["id"], first);
    let payload = json!({"sourceId":first,"state":"ordered","purchaseNo":"tb001","actualCost":12,"confirmed":true});
    let _ = fulfillment::purchase(State(f.state.clone()), Path(id), Json(payload.clone()))
        .await
        .unwrap();
    let _ = fulfillment::purchase(State(f.state.clone()), Path(id), Json(payload))
        .await
        .unwrap();
    f.observe(&o);
    assert_eq!(f.orders()[0]["state"], "awaiting_tracking");
    assert_eq!(
        fulfillment::purchase_list(&lock(&f.state), id)
            .unwrap()
            .len(),
        1
    );
    assert!(fulfillment::purchase(
        State(f.state.clone()),
        Path(id),
        Json(json!({"sourceId":backup,"state":"draft"}))
    )
    .await
    .is_err());
    o["refund_status"] = json!(1);
    o["update_time"] = json!(101);
    f.observe(&o);
    assert_eq!(f.orders()[0]["state"], "after_sale");
    assert!(fulfillment::purchase(State(f.state.clone()),Path(id),Json(json!({"sourceId":first,"state":"ordered","confirmed":true,"purchaseNo":"tb002","actualCost":12}))).await.is_err());
    let _ = fulfillment::purchase(
        State(f.state.clone()),
        Path(id),
        Json(
            json!({"sourceId":first,"state":"canceled","confirmed":true,"supplierRefund":12,"note":"已在淘宝完成退款"}),
        ),
    )
    .await
    .unwrap();
    let _ = fulfillment::note(
        State(f.state.clone()),
        Path(id),
        Json(json!({"note":"销售与采购已核查","close":true})),
    )
    .await
    .unwrap();
    f.observe(&o);
    assert_eq!(f.orders()[0]["state"], "closed");
    o["refund_status"] = json!(5);
    o["refund_amount"] = json!(5000);
    o["update_time"] = json!(102);
    f.observe(&o);
    assert_eq!(f.orders()[0]["state"], "after_sale");
    o["refund_status"] = json!(4);
    o["update_time"] = json!(103);
    f.observe(&o);
    let _ = fulfillment::purchase(
        State(f.state.clone()),
        Path(id),
        Json(json!({"sourceId":backup,"state":"draft"})),
    )
    .await
    .unwrap();
    assert_eq!(
        fulfillment::purchase_list(&lock(&f.state), id)
            .unwrap()
            .len(),
        2
    );
}
#[tokio::test]
async fn unknown_shipping_survives_sync_and_outdated_orders_are_ignored() {
    let f = Fixture::new();
    let bid = f.book("书");
    f.channel(bid, "mixed");
    let mut o = order("ship", "9007199254741001");
    f.observe(&o);
    lock(&f.state)
        .execute("UPDATE fulfillment_orders SET state='shipping_unknown'", [])
        .unwrap();
    f.observe(&o);
    assert_eq!(f.orders()[0]["state"], "shipping_unknown");
    o["update_time"] = json!(101);
    o["order_status"] = json!(21);
    f.observe(&o);
    assert_eq!(f.orders()[0]["state"], "shipped");
    o["update_time"] = json!(99);
    o["order_status"] = json!(12);
    f.observe(&o);
    assert_eq!(f.orders()[0]["state"], "shipped");
}
#[tokio::test]
async fn digital_delivery_requires_evidence_and_preserves_resource_snapshot() {
    let f = Fixture::new();
    let bid = f.book("书");
    f.channel(bid, "mixed");
    let r=add_resource(State(f.state.clone()),Path(bid),Json(json!({"version":"v1","url":"https://example.org/one","instructions":"下载","code":"1234"}))).await.unwrap().0["id"].as_i64().unwrap();
    lock(&f.state)
        .execute(
            "UPDATE channel_products SET data=json_set(data,'$.resourceId',?)",
            [r],
        )
        .unwrap();
    let mut o = order("digital", "9007199254741002");
    o["goods"]["quantity"] = json!(2);
    f.observe(&o);
    lock(&f.state)
        .execute(
            "UPDATE channel_products SET data=json_remove(data,'$.resourceId')",
            [],
        )
        .unwrap();
    f.observe(&o);
    assert_eq!(f.orders()[0]["data"]["resource"]["version"], "v1");
    for cards in [
        json!({"list":[]}),
        json!({"list":[{"sold_type":21,"card_no":"one"}]}),
        json!({"list":[{"sold_type":11,"card_no":"one"}]}),
    ] {
        record_delivery(&lock(&f.state), "test-app", "digital", &cards).unwrap();
        assert_eq!(f.orders()[0]["state"], "delivery_pending");
    }
    let cards = json!({"list":[{"sold_type":11,"card_no":"one"},{"sold_type":11,"card_no":"two"}]});
    record_delivery(&lock(&f.state), "test-app", "digital", &cards).unwrap();
    record_delivery(&lock(&f.state), "test-app", "digital", &cards).unwrap();
    assert_eq!(f.orders()[0]["state"], "delivered");
    assert_eq!(
        lock(&f.state)
            .query_row("SELECT count(*) FROM delivery_records", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db::get_book(&lock(&f.state), bid).unwrap().unwrap().stock,
        7
    );
    o["order_status"] = json!(24);
    o["update_time"] = json!(101);
    f.observe(&o);
    assert_eq!(f.orders()[0]["state"], "canceled_review");
}
#[tokio::test]
async fn one_channel_per_book_shop_and_mixed_publish_gate() {
    let f = Fixture::new();
    let bid = f.book("书");
    source(&f, bid, true, 10.0).await;
    let payload = json!({"bookId":bid,"shop":"shop","data":{"mode":"mixed","title":"书","physicalPrice":20,"digitalPrice":5}});
    let ch = save_channel(State(f.state.clone()), Json(payload.clone()))
        .await
        .unwrap()
        .0;
    let _ = save_channel(State(f.state.clone()), Json(payload.clone()))
        .await
        .unwrap();
    let check = check_channel(&lock(&f.state), ch["id"].as_i64().unwrap()).unwrap();
    assert_eq!(check["ready"], false);
    assert_eq!(capability()["canPublishMixed"], false);
    assert_eq!(channel_list(&lock(&f.state), bid).unwrap().len(), 1);
    let mut batch = payload;
    batch["createOnly"] = json!(true);
    assert!(save_channel(State(f.state.clone()), Json(batch))
        .await
        .is_err());
}
#[test]
fn backup_and_idempotent_migration_preserve_old_book_and_capture() {
    let f = Fixture::new();
    let input:CaptureInput=serde_json::from_value(json!({"itemId":"legacy","title":"旧书","url":"https://item.taobao.com/item.htm?id=1","price":12})).unwrap();
    let (_, book, _) = db::upsert_capture(&lock(&f.state), &input, 0.3, 5.0, "九成新").unwrap();
    let bid = book.unwrap().id;
    {
        let c = lock(&f.state);
        c.execute("DROP TABLE commerce_meta", []).unwrap();
        schema::backup(&c, &f.path).unwrap();
        schema::init(&c).unwrap();
        schema::init(&c).unwrap();
        assert_eq!(source_list(&c, Some(bid)).unwrap().len(), 1);
        assert_eq!(db::get_book(&c, bid).unwrap().unwrap().title, "旧书");
    }
    let backups = fs::read_dir(&f.path)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("shelf-before-commerce")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(backups[0].path()).unwrap();
    assert_eq!(
        backup
            .query_row("SELECT title FROM books WHERE id=?", [bid], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "旧书"
    );
}

#[tokio::test]
async fn invalid_resources_create_tasks_and_finance_never_assumes_zero_cost() {
    let f = Fixture::new();
    let bid = f.book("书");
    let sid = source(&f, bid, true, 10.0).await;
    f.channel(bid, "mixed");
    f.observe(&order("finance", "9007199254741001"));
    let id = f.orders()[0]["id"].as_i64().unwrap();
    let m = overview(State(f.state.clone())).await.unwrap().0;
    assert_eq!(m["metrics"]["physical"]["revenue"], 5000);
    assert_eq!(m["metrics"]["physical"]["pendingCost"], 1);
    assert_eq!(m["metrics"]["physical"]["margin"], 0);
    let _=fulfillment::purchase(State(f.state.clone()),Path(id),Json(json!({"sourceId":sid,"state":"ordered","actualCost":12,"purchaseNo":"tb-finance","confirmed":true}))).await.unwrap();
    assert!(fulfillment::purchase(
        State(f.state.clone()),
        Path(id),
        Json(json!({"sourceId":sid,"state":"draft"}))
    )
    .await
    .is_err());
    let _ = fulfillment::finance(
        State(f.state.clone()),
        Path(id),
        Json(json!({"fees":2,"refund":5})),
    )
    .await
    .unwrap();
    let m = overview(State(f.state.clone())).await.unwrap().0;
    assert_eq!(m["metrics"]["physical"]["margin"], 3100);
    assert_eq!(m["metrics"]["physical"]["pendingCost"], 0);
    let _=fulfillment::purchase(State(f.state.clone()),Path(id),Json(json!({"sourceId":sid,"state":"canceled","supplierRefund":10,"note":"供应商扣除运费后退款","confirmed":true}))).await.unwrap();
    let m = overview(State(f.state.clone())).await.unwrap().0;
    assert_eq!(m["metrics"]["physical"]["cost"], 200);
    let res = add_resource(
        State(f.state.clone()),
        Path(bid),
        Json(json!({"version":"v1","url":"https://example.org/resource","instructions":"下载"})),
    )
    .await
    .unwrap()
    .0;
    let _ = resource_state(
        State(f.state.clone()),
        Path(res["id"].as_i64().unwrap()),
        Json(json!({"state":"invalid"})),
    )
    .await
    .unwrap();
    let m = overview(State(f.state.clone())).await.unwrap().0;
    assert!(m["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| text(t, "reason").contains("资源失效")));
}
