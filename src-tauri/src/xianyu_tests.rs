use super::*;
use axum::{body::Bytes, extract::Query, http::StatusCode};
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

#[derive(Clone)]
struct Mock {
    mode: &'static str,
    requests: Arc<StdMutex<Vec<(String, Value)>>>,
}
async fn remote(
    State(mock): State<Mock>,
    uri: axum::http::Uri,
    Query(query): Query<HashMap<String, String>>,
    raw: Bytes,
) -> (StatusCode, Json<Value>) {
    let body: Value = serde_json::from_slice(&raw).unwrap();
    let timestamp = query.get("timestamp").unwrap();
    assert_eq!(query.get("appid").unwrap(), "12345");
    assert_eq!(
        query.get("sign").unwrap(),
        &sign(
            "12345",
            "test-secret",
            timestamp,
            std::str::from_utf8(&raw).unwrap()
        )
    );
    let path = uri.path();
    mock.requests
        .lock()
        .unwrap()
        .push((path.to_string(), body.clone()));
    let data = match path {
        "/api/open/user/authorize/list" => {
            json!({"list":[{"user_name":"seller","is_valid":true,"authorize_expires":4_000_000_000_i64}]})
        }
        "/api/open/product/create" => {
            if mock.mode == "ambiguous" {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"code":500,"msg":"temporary error"})),
                );
            }
            if mock.mode == "internal-error" {
                return (
                    StatusCode::OK,
                    Json(json!({"code":500,"msg":"Internal Server Error"})),
                );
            }
            if mock.mode == "slow" {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            if mock.mode == "reject"
                || (mock.mode == "fallback"
                    && body["publish_shop"][0]["images"][0]
                        .as_str()
                        .unwrap()
                        .starts_with("data:"))
            {
                return (
                    StatusCode::OK,
                    Json(json!({"code":400,"msg":"images 图片格式不支持"})),
                );
            }
            json!({"product_id":9007199254740993_u64,"product_status":21})
        }
        "/api/open/product/publish" => json!({}),
        "/api/open/order/list" if mock.mode == "commerce-pages" => {
            let page = body["page_no"].as_i64().unwrap();
            let count = if page == 1 { 100 } else { 1 };
            let list: Vec<Value> = (0..count)
                .map(|i| {
                    let mut o = commerce_order();
                    o["order_no"] = json!(format!("{}", (page - 1) * 100 + i + 1));
                    o
                })
                .collect();
            json!({"list":list,"count":101})
        }
        "/api/open/order/list" => {
            json!({"list":[if mock.mode.starts_with("commerce"){commerce_order()}else{mock_order()}],"count":1})
        }
        "/api/open/order/detail" if mock.mode.starts_with("commerce") => {
            let mut o = commerce_order();
            if mock.mode == "commerce-physical"
                && mock
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(p, _)| p == "/api/open/order/ship")
            {
                o["order_status"] = json!(21);
                o["update_time"] = json!(101);
            }
            o
        }
        "/api/open/order/detail" => mock_order(),
        "/api/open/order/ship" if mock.mode == "commerce-timeout" => {
            tokio::time::sleep(Duration::from_millis(200)).await;
            json!({})
        }
        "/api/open/order/ship" => json!({}),
        "/api/open/product/sku/list" => {
            json!({"list":[{"product_id":9007199254740993u64,"sku_items":[{"sku_id":9007199254741001u64,"stock":1,"sku_text":"实体版","price":2000},{"sku_id":9007199254741002u64,"stock":2,"sku_text":"电子版","price":500}]}]})
        }
        "/api/open/product/detail" => {
            if mock.mode == "stock-error" {
                return (
                    StatusCode::OK,
                    Json(json!({"code":100004,"msg":"库存查询失败"})),
                );
            }
            json!({"product_id":9007199254740993_u64,"stock":3,"spec_type":2,"product_status":22,"publish_status":3,"publish_shop":[{"user_name":"seller","status":1,"item_id":9007199254740995_u64}]})
        }
        "/api/open/product/list" => {
            json!({"list":[{"product_id":9007199254740993_u64,"outer_id":"taoshu-1-1"}],"count":1})
        }
        _ => json!({}),
    };
    (
        StatusCode::OK,
        Json(json!({"code":0,"msg":"OK","data":data})),
    )
}
struct Fixture {
    state: AppState,
    mock: Mock,
    server: tokio::task::JoinHandle<()>,
    dir: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort(); /* SQLite handles close with state; fixture files remain under OS temp. */
    }
}
async fn fixture(mode: &'static str) -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "taoshu-xianyu-test-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let mut state = AppState::new(dir.clone(), 0).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mock = Mock {
        mode,
        requests: Arc::new(StdMutex::new(Vec::new())),
    };
    let router = Router::new()
        .fallback(post(remote))
        .with_state(mock.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut gateway = Gateway::new().unwrap();
    gateway.base = base;
    if mode == "slow" || mode == "commerce-timeout" {
        gateway.client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
    }
    state.xianyu = Arc::new(gateway);
    {
        let conn = lock(&state);
        db::set_setting(&conn, "xianyuAppKey", &json!("12345")).unwrap();
        db::set_setting(&conn, "xianyuAppSecret", &json!("test-secret")).unwrap();
        let patch = serde_json::from_value(
            json!({"title":"测试图书","isbn":"9787505720176","sellPrice":12.34,"stock":1}),
        )
        .unwrap();
        let book = db::create_book(&conn, &patch).unwrap();
        // Distinct files prove that the processed image is preferred.
        std::fs::write(state.image_dir.join("original.png"), b"original").unwrap();
        std::fs::write(state.image_dir.join("processed.png"), b"processed").unwrap();
        let image = db::add_image(
            &conn,
            book.id,
            "upload",
            "",
            "original.png",
            Some(1),
            Some(1),
        )
        .unwrap();
        db::set_processed(&conn, image.id, "processed.png", Some(1), Some(1)).unwrap();
    }
    Fixture {
        state,
        mock,
        server,
        dir,
    }
}
fn input() -> PublishInput {
    serde_json::from_value(json!({"userName":"seller","channelCatId":"book-cat","province":110000,"city":110100,"district":110101,"title":"测试图书","description":"一本测试用的图书","price":12.34,"expressFee":1.01,"stock":1,"stuffStatus":90,"createOnly":true,"firstImageOnly":true})).unwrap()
}

#[test]
fn isbn_matches_documented_formats_and_normalizes_separators() {
    assert_eq!(
        normalize_isbn(" 978-7-5057-2017-6 ").unwrap(),
        "9787505720176"
    );
    assert_eq!(normalize_isbn("0 8044 2957 x").unwrap(), "080442957X");
    assert!(normalize_isbn("9791234567890").is_ok());
    for value in [
        "",
        " \t-",
        "978123",
        "9771234567890",
        "978750572017X",
        "123X567890",
    ] {
        assert!(normalize_isbn(value).is_err(), "accepted {value:?}");
    }
}

#[tokio::test]
async fn missing_or_invalid_isbn_is_rejected_before_images_or_remote_calls() {
    let f = fixture("ok").await;
    lock(&f.state)
        .execute("UPDATE books SET isbn='' WHERE id=1", [])
        .unwrap();
    for value in [None, Some(""), Some("bad-isbn")] {
        let mut request = input();
        request.isbn = value.map(str::to_string);
        // This would require Gitee configuration and upload if ISBN validation ran too late.
        request.image_mode = "gitee".into();
        let error = publish_book(State(f.state.clone()), Path(1), Json(request))
            .await
            .unwrap_err();
        assert!(format!("{error:?}").contains("ISBN"));
        assert!(f.mock.requests.lock().unwrap().is_empty());
    }
    let count: i64 = lock(&f.state)
        .query_row("SELECT count(*) FROM xianyu_tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn supplemented_isbn_is_sent_when_local_book_has_none() {
    let f = fixture("ok").await;
    lock(&f.state)
        .execute("UPDATE books SET isbn='' WHERE id=1", [])
        .unwrap();
    let mut request = input();
    request.isbn = Some(" 978-7-5057-2017-6 ".into());
    let result = publish_book(State(f.state.clone()), Path(1), Json(request))
        .await
        .unwrap()
        .0;
    assert_eq!(result["status"], "created");
    let requests = f.mock.requests.lock().unwrap();
    let (_, payload) = requests
        .iter()
        .find(|(path, _)| path.ends_with("/create"))
        .unwrap();
    assert_eq!(payload["book_data"]["isbn"], "9787505720176");
}

#[tokio::test]
async fn invalid_region_relationship_is_rejected_before_remote_calls() {
    let f = fixture("ok").await;
    let mut request = input();
    request.district = 440305;
    assert!(publish_book(State(f.state.clone()), Path(1), Json(request))
        .await
        .is_err());
    assert!(f.mock.requests.lock().unwrap().is_empty());
}

#[test]
fn documented_signature_and_utf8_bytes() {
    assert_eq!(
        sign(
            "203413189371893",
            "o9wl81dncmvby3ijpq7eur456zhgtaxs",
            "1636087298",
            r#"{"product_id":"219530767978565"}"#
        ),
        "c26c8a48809141f3dd80bd9b9ddb41ea"
    );
    let raw = r#"{"title":"测试图书","url":"https://example.com/a?b=1"}"#;
    assert_ne!(
        sign("1", "secret", "1", raw),
        sign(
            "1",
            "secret",
            "1",
            &raw.replace("测试图书", "\\u6d4b\\u8bd5\\u56fe\\u4e66")
        )
    );
}
#[test]
fn money_and_ids_remain_exact() {
    assert_eq!(money(12.34, false).ok(), Some(1234));
    assert_eq!(money(1.01, false).ok(), Some(101));
    assert!(money(f64::NAN, false).is_err());
    assert!(money(-1.0, true).is_err());
    assert!(money(0.0, false).is_err());
    assert_eq!(money(0.0, true).ok(), Some(0));
    let mut body = json!({"product_id":"9007199254740993"});
    integer_id(&mut body, "product_id").unwrap();
    assert_eq!(body["product_id"].as_u64(), Some(9007199254740993));
    browser_ids(&mut body);
    assert_eq!(body["product_id"], "9007199254740993");
}
#[test]
fn operation_allowlist_and_validation() {
    assert!(operation_body("arbitrary-url", json!({})).is_err());
    assert!(operation_body("stock", json!({"product_id":"1","stock":-1})).is_err());
    assert!(operation_body("products", json!({"page_no":101})).is_err());
    assert!(operation_body("ship",json!({"order_no":"123","waybill_no":"SF1234567890","express_code":"other","express_name":""})).is_err());
    let (_, payload) = operation_body(
        "product",
        json!({"product_id":"9007199254740993","access_token":"must-not-pass"}),
    )
    .unwrap();
    assert!(payload.get("access_token").is_none());
}
#[tokio::test]
async fn data_url_rejection_falls_back_and_reuses_processed_image() {
    let f = fixture("fallback").await;
    let result = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap()
        .0;
    assert_eq!(result["status"], "created");
    assert_eq!(result["imageMode"], "raw-base64");
    assert_eq!(result["productId"], "9007199254740993");
    assert_eq!(result["attempts"].as_array().unwrap().len(), 2);
    let requests = f.mock.requests.lock().unwrap();
    let creates: Vec<_> = requests
        .iter()
        .filter(|(p, _)| p.ends_with("/create"))
        .collect();
    assert_eq!(creates.len(), 2);
    assert_eq!(
        creates[0].1["publish_shop"][0]["images"][0],
        "data:image/png;base64,cHJvY2Vzc2Vk"
    );
    assert_eq!(creates[1].1["publish_shop"][0]["images"][0], "cHJvY2Vzc2Vk");
    assert_eq!(creates[1].1["book_data"]["isbn"], "9787505720176");
    assert_eq!(creates[1].1["price"], 1234);
    assert_eq!(creates[1].1["express_fee"], 101);
    assert!(!requests.iter().any(|(p, _)| p.ends_with("/publish")));
    drop(requests);
    assert!(publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .is_err());
}
#[tokio::test]
async fn image_rejections_are_recorded_without_submitting_publish() {
    let f = fixture("reject").await;
    let result = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap()
        .0;
    assert_eq!(result["status"], "base64_rejected");
    assert_eq!(result["productId"], "");
    assert_eq!(result["attempts"].as_array().unwrap().len(), 2);
    assert!(!result.to_string().contains("cHJvY2Vzc2Vk"));
    assert!(!result.to_string().contains("test-secret"));
}
#[tokio::test]
async fn ambiguous_failure_is_not_retried_and_can_be_recovered() {
    let f = fixture("ambiguous").await;
    let result = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap()
        .0;
    assert_eq!(result["status"], "unknown");
    assert_eq!(
        f.mock
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("/create"))
            .count(),
        1
    );
    assert!(publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .is_err());
    refresh(&f.state, &config(&f.state).ok().unwrap(), 1)
        .await
        .unwrap();
    let recovered = task(&f.state, 1, "12345").ok().unwrap();
    assert_eq!(recovered.product_id, "9007199254740993");
    assert_eq!(recovered.status, "published");
}
#[tokio::test]
async fn timeout_is_unknown_and_does_not_attempt_raw_base64() {
    let f = fixture("slow").await;
    let result = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap()
        .0;
    assert_eq!(result["status"], "unknown");
    assert_eq!(result["imageMode"], "data-url");
    assert_eq!(
        f.mock
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("/create"))
            .count(),
        1
    );
}
#[tokio::test]
async fn accepted_publish_requires_remote_confirmation_and_manual_image_verification() {
    let f = fixture("ok").await;
    let mut request = input();
    request.create_only = false;
    let result = publish_book(State(f.state.clone()), Path(1), Json(request))
        .await
        .unwrap()
        .0;
    assert_eq!(result["status"], "publishing");
    assert_eq!(result["imagesVerified"], false);
    assert!(verify_images(State(f.state.clone()), Path(1))
        .await
        .is_err());
    let result = refresh_task(State(f.state.clone()), Path(1))
        .await
        .unwrap()
        .0;
    assert_eq!(result["status"], "published");
    assert_eq!(result["itemId"], "9007199254740995");
    assert_eq!(result["imagesVerified"], false);
    let result = verify_images(State(f.state.clone()), Path(1))
        .await
        .unwrap()
        .0;
    assert_eq!(result["imagesVerified"], true);
    assert_eq!(
        db::get_book(&lock(&f.state), 1).unwrap().unwrap().status,
        "listed"
    );
}
#[tokio::test]
async fn secrets_are_not_returned_and_routes_require_local_auth() {
    let f = fixture("ok").await;
    assert_eq!(
        config_get(State(f.state.clone())).await.0,
        json!({"appKey":"12345","secretConfigured":true})
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = crate::api::router(f.state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let response = reqwest::get(format!("{base}/api/xianyu/config"))
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let response = reqwest::Client::new()
        .get(format!("{base}/api/xianyu/config"))
        .header("x-kf-token", &f.state.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    server.abort();
}
#[tokio::test]
async fn additive_migration_preserves_existing_books_and_task_links() {
    let f = fixture("ok").await;
    let _ = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap();
    let conn = lock(&f.state);
    db::init(&conn).unwrap();
    assert!(db::get_book(&conn, 1).unwrap().is_some());
    conn.execute("DELETE FROM books WHERE id=1", []).unwrap();
    let task_book: Option<i64> = conn
        .query_row("SELECT book_id FROM xianyu_tasks WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(task_book, None);
    assert!(f.dir.starts_with(std::env::temp_dir()));
}

#[test]
fn diagnostics_do_not_persist_secrets_or_echoed_base64() {
    let raw = "cHJvY2Vzc2VkLWltYWdlLWJ5dGVzLWZvci10ZXN0aW5n";
    let body = json!({"publish_shop":[{"images":[raw]}]});
    let message = diagnostic(
        &format!("images invalid test-secret: {raw}"),
        "test-secret",
        &body,
    );
    assert!(!message.contains("test-secret"));
    assert!(!message.contains("cHJvY2Vzc2Vk"));
    assert!(message.contains("图片内容已省略"));
}
#[test]
fn remote_state_does_not_attribute_other_shop_sales_to_this_task() {
    let data = json!({"product_status":22,"publish_status":3,"publish_shop":[{"user_name":"other","status":1,"item_id":123}]});
    assert_ne!(remote_state(&data, "seller").0, Some("published"));
}

#[tokio::test]
async fn http_200_with_internal_error_is_still_an_unknown_creation() {
    let f = fixture("internal-error").await;
    let result = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap()
        .0;
    assert_eq!(result["status"], "unknown");
    assert_eq!(
        f.mock
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("/create"))
            .count(),
        1
    );
}

fn mock_order() -> Value {
    json!({"order_no":"3364202298717566229","order_status":12,"refund_status":0,
        "order_time":1685087039_i64,"pay_amount":1234,
        "receiver_mobile":"private-mobile","address":"private-address",
        "goods":{"product_id":9007199254740993_u64,"item_id":9007199254740995_u64,
        "quantity":2,"title":"测试图书","outer_id":"taoshu-1-1"}})
}

#[tokio::test]
async fn orders_link_by_exact_published_ids_and_cache_without_receiver_data() {
    let f = fixture("ok").await;
    let _ = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap();
    let listed = dispatch(
        State(f.state.clone()),
        Path("orders".into()),
        Json(json!({})),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(listed["list"][0]["localBook"]["id"], 1);
    assert_eq!(listed["list"][0]["goods"]["product_id"], "9007199254740993");
    assert!(!f
        .mock
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|(p, _)| p.ends_with("/product/detail")));
    let mut order = mock_order();
    order["goods"]["product_id"] = Value::Null;
    // The exact outer_id is an independent match to an actual publishing task.
    orders::enrich(&f.state, "12345", &mut order).unwrap();
    assert_eq!(order["localBook"]["id"], 1);
    orders::enrich(&f.state, "different-app", &mut order).unwrap();
    assert!(order["localBook"].is_null());
    let conn = lock(&f.state);
    let cached: String = conn
        .query_row(
            "SELECT summary FROM xianyu_orders WHERE app_key='12345'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!cached.contains("private-mobile"));
    assert!(!cached.contains("private-address"));
    assert_eq!(db::get_book(&conn, 1).unwrap().unwrap().stock, 1);
}

#[tokio::test]
async fn order_detail_queries_live_stock_on_every_open_and_leaves_local_stock_unchanged() {
    let f = fixture("ok").await;
    let _ = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap();
    for _ in 0..2 {
        let detail = dispatch(
            State(f.state.clone()),
            Path("order".into()),
            Json(json!({"order_no":"3364202298717566229"})),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(detail["remoteInventory"]["stock"], 3);
        assert_eq!(detail["localBook"]["stock"], 1);
        assert_eq!(detail["goods"]["quantity"], 2);
        assert_eq!(detail["bookLinkSource"], "auto");
    }
    assert_eq!(
        f.mock
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("/product/detail"))
            .count(),
        2
    );
    let history = orders::book_orders(State(f.state.clone()), Path(1))
        .await
        .unwrap()
        .0;
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["order_no"], "3364202298717566229");
    assert_eq!(db::get_book(&lock(&f.state), 1).unwrap().unwrap().stock, 1);
}

#[tokio::test]
async fn inventory_failure_keeps_order_detail_available() {
    let f = fixture("stock-error").await;
    let detail = dispatch(
        State(f.state.clone()),
        Path("order".into()),
        Json(json!({"order_no":"3364202298717566229"})),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(detail["order_no"], "3364202298717566229");
    assert!(detail["remoteInventory"]["stock"].is_null());
    assert!(detail["remoteInventory"]["error"]
        .as_str()
        .unwrap()
        .contains("库存查询失败"));
}

#[tokio::test]
async fn manual_order_link_and_unlink_survive_refresh_and_book_deletion() {
    let f = fixture("ok").await;
    let no = "3364202298717566229";
    let input = serde_json::from_value(json!({"bookId":1})).unwrap();
    let linked = orders::link_book(State(f.state.clone()), Path(no.into()), Json(input))
        .await
        .unwrap()
        .0;
    assert_eq!(linked["localBook"]["id"], 1);
    let mut order = mock_order();
    orders::enrich(&f.state, "12345", &mut order).unwrap();
    assert_eq!(order["bookLinkSource"], "manual");
    assert_eq!(order["localBook"]["id"], 1);
    let input = serde_json::from_value(json!({"bookId":null})).unwrap();
    let _ = orders::link_book(State(f.state.clone()), Path(no.into()), Json(input))
        .await
        .unwrap();
    orders::enrich(&f.state, "12345", &mut order).unwrap();
    assert!(order["localBook"].is_null());
    assert!(orders::book_orders(State(f.state.clone()), Path(1))
        .await
        .unwrap()
        .0
        .as_array()
        .unwrap()
        .is_empty());
    let input = serde_json::from_value(json!({"bookId":999})).unwrap();
    assert!(
        orders::link_book(State(f.state.clone()), Path(no.into()), Json(input))
            .await
            .is_err()
    );
    let input = serde_json::from_value(json!({"bookId":1})).unwrap();
    let _ = orders::link_book(State(f.state.clone()), Path(no.into()), Json(input))
        .await
        .unwrap();
    lock(&f.state)
        .execute("DELETE FROM books WHERE id=1", [])
        .unwrap();
    orders::enrich(&f.state, "12345", &mut order).unwrap();
    assert!(order["localBook"].is_null());
}

#[tokio::test]
async fn conflicting_ids_or_similar_titles_do_not_guess_a_local_book() {
    let f = fixture("ok").await;
    let _ = publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .unwrap();
    let mut order = mock_order();
    order["goods"] =
        json!({"title":"测试图书","product_id":0,"item_id":0,"outer_id":"taoshu-1-999"});
    orders::enrich(&f.state, "12345", &mut order).unwrap();
    assert!(order["localBook"].is_null());
    {
        let conn = lock(&f.state);
        let patch = serde_json::from_value(json!({"title":"测试图书"})).unwrap();
        let b = db::create_book(&conn, &patch).unwrap();
        conn.execute("INSERT INTO xianyu_tasks(book_id,app_key,user_name,title,product_id,item_id,image_mode,created_at,updated_at) VALUES(?,'12345','seller','same','77','88','url','now','now')",[b.id]).unwrap();
        conn.execute("UPDATE xianyu_tasks SET item_id='99' WHERE id=1", [])
            .unwrap();
    }
    order["goods"] = json!({"product_id":9007199254740993_u64,"item_id":88});
    orders::enrich(&f.state, "12345", &mut order).unwrap();
    assert!(order["localBook"].is_null());
    order["goods"] = json!({"item_id":99});
    orders::enrich(&f.state, "12345", &mut order).unwrap();
    assert_eq!(order["localBook"]["id"], 1);
}

fn commerce_order() -> Value {
    let mut o = mock_order();
    o["seller_name"] = json!("seller");
    o["goods"]["sku_id"] = json!(9007199254741001u64);
    o["goods"]["quantity"] = json!(1);
    o["update_time"] = json!(100);
    o
}
async fn commerce_http(f: &Fixture) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = crate::api::router(f.state.clone());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, handle)
}
async fn commerce_post(
    f: &Fixture,
    base: &str,
    path: &str,
    payload: Value,
) -> (reqwest::StatusCode, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/commerce{path}"))
        .header("x-kf-token", &f.state.token)
        .json(&payload)
        .send()
        .await
        .unwrap();
    let status = r.status();
    (status, r.json().await.unwrap())
}
fn commerce_channel(f: &Fixture) {
    lock(&f.state).execute("INSERT INTO channel_products(book_id,app_key,shop,product_id,data,updated_at) VALUES(1,'12345','seller','9007199254740993',?,'now')",[json!({"mode":"mixed","physicalSkuId":"9007199254741001","digitalSkuId":"9007199254741002","itemId":"9007199254740995"}).to_string()]).unwrap();
}
#[tokio::test]
async fn commerce_sku_stock_is_distinct_and_never_falls_back_to_total() {
    let f = fixture("ok").await;
    let cfg = config(&f.state).unwrap();
    let r =
        commerce_gateway::inventory(&f.state, &cfg, "9007199254740993", "9007199254741001").await;
    assert_eq!(r["stock"], 3);
    assert_eq!(r["skuStock"], 1);
    let r = commerce_gateway::inventory(&f.state, &cfg, "9007199254740993", "missing").await;
    assert_eq!(r["stock"], 3);
    assert!(r["skuStock"].is_null());
    assert!(!r["skuError"].as_str().unwrap().is_empty());
    assert!(f
        .mock
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("/sku/list"))
        .all(|(_, v)| v["product_id"] == json!([9007199254740993u64])));
}
#[tokio::test]
async fn commerce_order_pages_overlap_without_duplicate_procurement() {
    let f = fixture("commerce-pages").await;
    commerce_channel(&f);
    let (base, server) = commerce_http(&f).await;
    let unauthorized = reqwest::get(format!("{base}/api/commerce/overview"))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
    for _ in 0..2 {
        let (status, r) = commerce_post(&f, &base, "/sync-orders", json!({})).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{r}");
        assert_eq!(r["count"], 101);
    }
    let c = lock(&f.state);
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM fulfillment_orders WHERE state='to_purchase'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        101
    );
    assert!(db::get_setting(&c, "commerceOrderCursor:12345").is_some());
    server.abort();
}
#[tokio::test]
async fn commerce_mixed_and_existing_product_block_before_any_upload() {
    let f = fixture("ok").await;
    let mut request = input();
    request.sale_mode = "mixed".into();
    request.image_mode = "gitee".into();
    assert!(publish_book(State(f.state.clone()), Path(1), Json(request))
        .await
        .is_err());
    assert!(f.mock.requests.lock().unwrap().is_empty());
    commerce_channel(&f);
    assert!(publish_book(State(f.state.clone()), Path(1), Json(input()))
        .await
        .is_err());
    assert!(f.mock.requests.lock().unwrap().is_empty());
}
#[tokio::test]
async fn manual_and_taobao_shipping_only_send_logistics_and_never_procurement_to_the_platform() {
    for taobao in [false,true] {
        let f=fixture("commerce-physical").await;
        let (base,server)=commerce_http(&f).await;
        let (status,r)=commerce_post(&f,&base,"/sync-orders",json!({})).await;assert!(status.is_success(),"{r}");
        f.mock.requests.lock().unwrap().clear();
        let endpoint=if taobao {"/fulfillments/1/purchase"} else {"/fulfillments/1/tracking"};
        let payload=json!({"platform":"taobao","purchaseNo":"TB-PRIVATE-8877","actualCost":7.77,"shop":"PRIVATE-SUPPLIER","note":"PRIVATE-INTERNAL-NOTE","state":"shipped","trackingNo":"SF123456","expressCode":"SF","expressName":"顺丰","confirmed":true,"physicalConfirmed":true});
        let (status,r)=commerce_post(&f,&base,endpoint,payload).await;assert!(status.is_success(),"{r}");
        assert!(f.mock.requests.lock().unwrap().is_empty(),"saving an internal link must never call the platform");
        let (status,_)=commerce_post(&f,&base,"/fulfillments/1/ship",json!({})).await;assert!(!status.is_success());
        assert!(f.mock.requests.lock().unwrap().is_empty());
        let (status,r)=commerce_post(&f,&base,"/fulfillments/1/ship",json!({"confirmed":true,"purchaseNo":"TB-PRIVATE-8877","note":"PRIVATE-INTERNAL-NOTE"})).await;assert!(status.is_success(),"{r}");
        {
            let sent=f.mock.requests.lock().unwrap();
            let body=&sent.iter().find(|(path,_)|path=="/api/open/order/ship").unwrap().1;
            assert_eq!(body.as_object().unwrap().len(),4);
            assert_eq!(body["waybill_no"],"SF123456");assert_eq!(body["express_code"],"SF");
            for (_,body) in sent.iter() { let text=body.to_string();assert!(!text.contains("PRIVATE"));assert!(!text.contains("7.77")); }
        }
        let (_,r)=commerce_post(&f,&base,"/fulfillments/1/refresh",json!({})).await;
        assert_eq!(r["order"]["fulfillment"]["state"],"shipped");
        server.abort();
    }
}

#[tokio::test]
async fn commerce_physical_http_flow_and_shipping_timeout_do_not_duplicate_send() {
    for mode in ["commerce-physical", "commerce-timeout"] {
        let f = fixture(mode).await;
        commerce_channel(&f);
        let (base, server) = commerce_http(&f).await;
        let (status, r) = commerce_post(&f, &base, "/sync-orders", json!({})).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{r}");
        let(status,source)=commerce_post(&f,&base,"/sources",json!({"bookId":1,"primary":true,"data":{"url":"https://item.taobao.com/item.htm?id=1","sku":"实体版","status":"available","dropship":true,"price":10,"freight":0,"checkedAt":"now"}})).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{source}");
        let(status,r)=commerce_post(&f,&base,"/fulfillments/1/purchase",json!({"sourceId":source["id"],"state":"shipped","purchaseNo":"tb001","actualCost":10,"trackingNo":"SF123456","expressCode":"SF","expressName":"顺丰","confirmed":true})).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{r}");
        let (status, _) = commerce_post(&f, &base, "/fulfillments/1/ship", json!({"confirmed":true})).await;
        assert_eq!(status.is_success(), mode == "commerce-physical");
        if mode == "commerce-timeout" {
            let(status,_)=commerce_post(&f,&base,"/fulfillments/1/purchase",json!({"sourceId":source["id"],"state":"shipped","purchaseNo":"tb001","actualCost":10,"trackingNo":"SF123456","expressCode":"SF","expressName":"顺丰","confirmed":true})).await;
            assert!(
                !status.is_success(),
                "editing procurement must not clear uncertain shipping"
            );
        }

        let (status, r) = commerce_post(&f, &base, "/fulfillments/1/ship", json!({"confirmed":true})).await;
        if mode == "commerce-physical" {
            assert!(status.is_success());
            assert_eq!(r["status"], "already_shipped")
        } else {
            assert!(!status.is_success());
            assert!(r["error"].as_str().unwrap().contains("禁止盲目重发"));
        }
        assert_eq!(
            f.mock
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(p, _)| p == "/api/open/order/ship")
                .count(),
            1
        );
        server.abort();
    }
}
