use super::*;
use rusqlite::Connection;
use std::collections::BTreeSet;

fn id(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|v| v.to_string()))
        .filter(|v| v != "0")
        .unwrap_or_default()
}

// Only trust exact published identifiers, never the title or the capture's source item_id.
fn match_book(conn: &Connection, key: &str, goods: &Value) -> rusqlite::Result<Option<i64>> {
    let product = id(&goods["product_id"]);
    let item = id(&goods["item_id"]);
    let outer = goods["outer_id"].as_str().unwrap_or("");
    let mut stmt = conn.prepare(
        "SELECT DISTINCT t.book_id FROM xianyu_tasks t JOIN books b ON b.id=t.book_id
        WHERE t.app_key=? AND t.product_id<>'' AND
        ((?<>'' AND t.product_id=?) OR (?<>'' AND t.item_id=?) OR
         (?<>'' AND ?='taoshu-' || t.book_id || '-' || t.id))",
    )?;
    let candidates = stmt
        .query_map(
            params![key, product, product, item, item, outer, outer],
            |r| r.get::<_, i64>(0),
        )?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    Ok(if candidates.len() == 1 {
        candidates.first().copied()
    } else {
        None
    })
}

pub(super) fn enrich(state: &AppState, key: &str, order: &mut Value) -> Result<(), ApiError> {
    let order_no = id(&order["order_no"]);
    if order_no.is_empty() {
        return Err(ApiError::bad("订单缺少订单号，无法关联本地图书"));
    }
    let conn = lock(state);
    let saved: Option<(Option<i64>, bool)> = conn
        .query_row(
            "SELECT book_id,manual FROM xianyu_orders WHERE app_key=? AND order_no=?",
            params![key, order_no],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let manual = saved.map(|(_, m)| m).unwrap_or(false);
    let mut book_id = if manual {
        saved.and_then(|(id, _)| id)
    } else {
        match_book(&conn, key, &order["goods"])?
    };
    crate::commerce::sync_legacy_channels(&conn)?;
    crate::commerce::observe_order(&conn, key, order)?;
    if !manual {
        let mapped:Option<i64>=conn.query_row("SELECT book_id FROM fulfillment_orders WHERE app_key=? AND order_no=? AND kind<>'unknown'",params![key,order_no],|r|r.get(0)).optional()?.flatten();
        if mapped.is_some() {
            book_id = mapped;
        }
    }
    // Cache only the fields needed for the book's order history, without receiver/buyer data.
    let summary = json!({"order_no":order_no,"order_status":order["order_status"],
        "refund_status":order["refund_status"],"order_time":order["order_time"],
        "pay_amount":order["pay_amount"],"goods":order["goods"]});
    conn.execute("INSERT INTO xianyu_orders(app_key,order_no,book_id,manual,summary,updated_at) VALUES(?,?,?,?,?,?)
        ON CONFLICT(app_key,order_no) DO UPDATE SET book_id=excluded.book_id,summary=excluded.summary,updated_at=excluded.updated_at",
        params![key,order_no,book_id,manual,summary.to_string(),db::now()])?;
    order["localBook"] = match book_id {
        Some(id) => db::get_book(&conn,id)?.map(|b| json!({"id":b.id,"title":b.title,"isbn":b.isbn,"stock":b.stock,"status":b.status})).unwrap_or(Value::Null),
        None => Value::Null,
    };
    order["bookLinkSource"] = json!(if manual {
        "manual"
    } else if book_id.is_some() {
        "auto"
    } else {
        "unmatched"
    });
    Ok(())
}

pub(super) async fn inventory(state: &AppState, cfg: &ConfigInput, order: &mut Value) {
    order["remoteInventory"] = super::commerce_gateway::inventory(
        state,
        cfg,
        &id(&order["goods"]["product_id"]),
        &crate::commerce::remote_id(&order["goods"]["sku_id"]),
    )
    .await;
    if let Some(obj) = order["remoteInventory"].as_object_mut() {
        obj.remove("product");
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LinkInput {
    book_id: Option<i64>,
}

pub(super) async fn link_book(
    State(state): State<AppState>,
    Path(order_no): Path<String>,
    Json(input): Json<LinkInput>,
) -> Result<Json<Value>, ApiError> {
    let cfg = config(&state)?;
    // Fetch from the configured application rather than trusting an arbitrary client snapshot.
    let mut order = state
        .xianyu
        .call(
            &cfg,
            "/api/open/order/detail",
            &json!({"order_no":order_no}),
        )
        .await
        .map_err(|e| ApiError::bad(e.message))?;
    if id(&order["order_no"]) != order_no {
        return Err(ApiError::bad("返回的订单号不匹配"));
    }
    enrich(&state, &cfg.app_key, &mut order)?;
    {
        let conn = lock(&state);
        if let Some(id) = input.book_id {
            if db::get_book(&conn, id)?.is_none() {
                return Err(ApiError::bad("本地图书不存在"));
            }
        }
        conn.execute(
            "UPDATE xianyu_orders SET book_id=?,manual=1 WHERE app_key=? AND order_no=?",
            params![input.book_id, cfg.app_key, order_no],
        )?;
    }
    enrich(&state, &cfg.app_key, &mut order)?;
    inventory(&state, &cfg, &mut order).await;
    browser_ids(&mut order);
    Ok(Json(order))
}

pub(super) async fn book_orders(
    State(state): State<AppState>,
    Path(book_id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let key = db::get_setting(&lock(&state), "xianyuAppKey")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let conn = lock(&state);
    let mut stmt = conn.prepare("SELECT summary,updated_at FROM xianyu_orders WHERE app_key=? AND book_id=? ORDER BY updated_at DESC,order_no DESC")?;
    let list = stmt
        .query_map(params![key, book_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut list = list
        .into_iter()
        .map(|(raw, at)| {
            let mut v: Value = serde_json::from_str(&raw).unwrap_or(json!({}));
            v["syncedAt"] = json!(at);
            v
        })
        .collect::<Vec<_>>();
    list.sort_by_key(|v| std::cmp::Reverse(v["order_time"].as_i64().unwrap_or(0)));
    let mut data = json!(list);
    browser_ids(&mut data);
    Ok(Json(data))
}
