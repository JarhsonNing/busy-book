use super::*;
// A closed or rejected refund no longer blocks procurement; a new refund does.
pub fn refund_blocks(status: i64) -> bool {
    ![0, 4, 6].contains(&status)
}
fn order_state(kind: &str, status: i64, refund: i64) -> &'static str {
    if kind == "unknown" {
        "unmatched"
    } else if refund_blocks(refund) || status == 23 {
        "after_sale"
    } else if status == 24 {
        "canceled_review"
    } else if status == 11 {
        "waiting_payment"
    } else if kind == "physical" {
        match status {
            12 => "to_purchase",
            21 => "shipped",
            22 => "completed",
            _ => "unmatched",
        }
    } else if [12, 21, 22].contains(&status) {
        "delivery_pending"
    } else {
        "unmatched"
    }
}

pub fn all(conn: &Connection) -> rusqlite::Result<Vec<Value>> {
    rows(conn,"SELECT json_object('id',id,'orderNo',order_no,'bookId',book_id,'channelId',channel_id,'kind',kind,'state',state,'snapshot',json(snapshot),'data',json(data),'updatedAt',updated_at) FROM fulfillment_orders WHERE app_key=? ORDER BY updated_at DESC,id DESC",[app_key(conn)])
}
pub fn purchase_list(conn: &Connection, id: i64) -> rusqlite::Result<Vec<Value>> {
    rows(conn,"SELECT json_object('id',id,'state',state,'data',json(data),'createdAt',created_at,'updatedAt',updated_at) FROM purchase_attempts WHERE fulfillment_id=? ORDER BY id DESC",[id])
}
fn get(conn: &Connection, id: i64) -> Result<Value, ApiError> {
    all(conn)?
        .into_iter()
        .find(|v| v["id"] == id)
        .ok_or_else(|| ApiError::bad("履约订单不存在或属于其他应用"))
}

pub fn observe_order(conn: &Connection, key: &str, order: &Value) -> rusqlite::Result<()> {
    let no = remote_id(&order["order_no"]);
    if no.is_empty() {
        return Ok(());
    }
    let product = remote_id(&order["goods"]["product_id"]);
    let sku = remote_id(&order["goods"]["sku_id"]);
    let item = remote_id(&order["goods"]["item_id"]);
    let matches=rows(conn,"SELECT json_object('id',id,'bookId',book_id,'shop',shop,'data',json(data)) FROM channel_products WHERE app_key=? AND product_id=? AND product_id<>''",params![key,product])?;
    let matches: Vec<_> = matches
        .into_iter()
        .filter(|c| {
            (!text(order, "seller_name").is_empty()
                && text(order, "seller_name") == text(c, "shop"))
                || (!item.is_empty() && text(&c["data"], "itemId") == item)
        })
        .collect();
    let mut kind = "unknown";
    let mut bid = None;
    let mut cid = None;
    let mut data = json!({"reason":"未找到唯一的商品与规格映射"});
    if matches.len() == 1 {
        let ch = &matches[0];
        cid = ch["id"].as_i64();
        bid = ch["bookId"].as_i64();
        let d = &ch["data"];
        if !sku.is_empty() && sku == text(d, "physicalSkuId") {
            kind = "physical"
        } else if !sku.is_empty() && sku != "0" && sku == text(d, "digitalSkuId") {
            kind = "digital"
        }
        if kind == "digital" {
            let rid = d["resourceId"].as_i64().unwrap_or(0);
            let resource:Option<String>=conn.query_row("SELECT json_object('id',id,'version',version,'state',state,'data',json(data)) FROM digital_resources WHERE id=? AND book_id=?",params![rid,bid],|r|r.get(0)).optional()?;
            data["resource"] = resource.map(parse).unwrap_or(Value::Null);
        }
        if kind == "physical" {
            let source = source_list(conn, bid)?.into_iter().find(|v| {
                v["primary"] == true
                    && text(&v["data"], "status") == "available"
                    && v["data"]["dropship"] == true
            });
            data["source"] = source.unwrap_or(Value::Null);
        }
    }
    if kind == "unknown" {
        bid = None;
        cid = None;
    }
    let status = order["order_status"].as_i64().unwrap_or(0);
    let refund = order["refund_status"].as_i64().unwrap_or(0);
    let state = order_state(kind, status, refund);
    let old:Option<(i64,String,String,String,String)>=conn.query_row("SELECT id,kind,state,snapshot,data FROM fulfillment_orders WHERE app_key=? AND order_no=?",params![key,no],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let mut snapshot = order.clone();
    if let Some(obj) = snapshot.as_object_mut() {
        obj.retain(|k, _| {
            [
                "order_no",
                "order_status",
                "refund_status",
                "refund_amount",
                "refund_time",
                "seller_name",
                "seller_eid",
                "order_time",
                "update_time",
                "pay_amount",
                "goods",
                "waybill_no",
                "express_code",
            ]
            .contains(&k.as_str())
        });
    }
    snapshot["order_no"] = json!(no);
    for field in ["product_id", "item_id", "sku_id"] {
        snapshot["goods"][field] = json!(remote_id(&order["goods"][field]));
    }
    if let Some((id, old_kind, old_state, old_snapshot, old_data)) = old {
        let prior = parse(old_snapshot);
        if prior["update_time"].as_i64().unwrap_or(0)
            > snapshot["update_time"].as_i64().unwrap_or(i64::MAX)
        {
            return Ok(());
        }
        if old_kind != "unknown" {
            data = parse(old_data);
            kind = if old_kind == "physical" {
                "physical"
            } else {
                "digital"
            };
        }
        if [21, 22].contains(&status) {
            data["shippingUncertain"] = json!(false);
        }
        let state = order_state(kind, status, refund);
        let closure_same = data["closure"]
            == json!([
                status,
                refund,
                snapshot["refund_amount"],
                snapshot["refund_time"]
            ]);
        let state = if old_state == "closed" && closure_same {
            "closed".to_string()
        } else if ["after_sale", "canceled_review"].contains(&state) {
            state.to_string()
        } else if data["shippingUncertain"] == true && kind == "physical" && status == 12 {
            "shipping_unknown".to_string()
        } else if old_kind != "unknown"
            && kind == "physical"
            && status == 12
            && [
                "purchased",
                "awaiting_tracking",
                "ready_to_ship",
                "shipping_unknown",
            ]
            .contains(&old_state.as_str())
        {
            old_state
        } else if kind == "digital"
            && old_state == "delivered"
            && !refund_blocks(refund)
            && [12, 21, 22].contains(&status)
        {
            old_state
        } else {
            state.to_string()
        };
        conn.execute("UPDATE fulfillment_orders SET book_id=COALESCE(book_id,?),channel_id=COALESCE(channel_id,?),kind=?,state=?,snapshot=?,data=?,updated_at=? WHERE id=?",params![bid,cid,kind,state,snapshot.to_string(),data.to_string(),db::now(),id])?;
    } else {
        conn.execute("INSERT INTO fulfillment_orders(app_key,order_no,book_id,channel_id,kind,state,snapshot,data,updated_at) VALUES(?,?,?,?,?,?,?,?,?)",params![key,no,bid,cid,kind,state,snapshot.to_string(),data.to_string(),db::now()])?;
    }
    Ok(())
}
pub fn record_delivery(
    conn: &Connection,
    key: &str,
    no: &str,
    cards: &Value,
) -> rusqlite::Result<()> {
    let found: Option<(i64, String, String, String)> = conn
        .query_row(
            "SELECT id,kind,snapshot,data FROM fulfillment_orders WHERE app_key=? AND order_no=?",
            params![key, no],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((id, kind, snapshot, data)) = found else {
        return Ok(());
    };
    if kind != "digital" {
        return Ok(());
    }
    let mut list = cards["list"].as_array().cloned().unwrap_or_default();
    list.sort_by_key(|v| v.to_string());
    list.dedup();
    let snapshot = parse(snapshot);
    let data = parse(data);
    let quantity = snapshot["goods"]["quantity"].as_u64().unwrap_or(0);
    let delivered = quantity > 0
        && list
            .iter()
            .filter(|v| matches!(v["sold_type"].as_i64(), Some(11 | 12)))
            .count() as u64
            >= quantity;
    if delivered && [12, 21, 22, 23].contains(&snapshot["order_status"].as_i64().unwrap_or(0)) {
        let fingerprint = format!("{:x}", md5::compute(json!(list).to_string()));
        conn.execute("INSERT OR IGNORE INTO delivery_records(fulfillment_id,fingerprint,data,created_at) VALUES(?,?,?,?)",params![id,fingerprint,json!({"cards":list,"resourceSnapshot":data["resource"],"resourceVersionVerified":false,"checkedAt":db::now()}).to_string(),db::now()])?;
        conn.execute("UPDATE fulfillment_orders SET state=CASE WHEN state IN ('after_sale','canceled_review','closed') THEN state ELSE 'delivered' END,updated_at=? WHERE id=?",params![db::now(),id])?;
    }
    Ok(())
}
pub(super) async fn list(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(all(&lock(&s))?)))
}
pub(super) async fn detail(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let mut v = get(&c, id)?;
    v["purchases"] = json!(purchase_list(&c, id)?);
    v["deliveries"]=json!(rows(&c,"SELECT json_object('id',id,'data',json(data),'createdAt',created_at) FROM delivery_records WHERE fulfillment_id=? ORDER BY id DESC",[id])?);
    v["events"]=json!(rows(&c,"SELECT json_object('action',action,'data',json(data),'createdAt',created_at) FROM commerce_events WHERE entity='fulfillment' AND entity_id=? ORDER BY id DESC",[id.to_string()])?);
    Ok(Json(v))
}
pub(super) async fn purchase(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let mut c = lock(&s);
    let tx = c.transaction()?;
    let f = get(&tx, id)?;
    if f["kind"] != "physical" {
        return Err(ApiError::bad("只有已确认实体规格的订单可采购"));
    }
    if f["state"] == "shipping_unknown" || f["data"]["shippingUncertain"] == true {
        return Err(ApiError::bad(
            "发货结果尚未核实，不能改写采购进度以再次发货，请先核查远端",
        ));
    }
    let state = required(&v, "state")?;
    if !["draft", "ordered", "shipped", "canceled"].contains(&state.as_str()) {
        return Err(ApiError::bad("采购状态无效"));
    }
    let order_status = f["snapshot"]["order_status"].as_i64().unwrap_or(0);
    if state != "canceled"
        && (![12, 21, 22].contains(&order_status)
            || refund_blocks(f["snapshot"]["refund_status"].as_i64().unwrap_or(0)))
    {
        return Err(ApiError::bad(
            "未付款、取消或售后订单不能继续采购，请先核查订单",
        ));
    }
    if state != "canceled" && f["snapshot"]["goods"]["quantity"].as_u64().unwrap_or(0) == 0 {
        return Err(ApiError::bad("订单购买数量未知，须先核查远端订单"));
    }
    let attempts = purchase_list(&tx, id)?;
    let current = attempts
        .iter()
        .find(|a| !["canceled", "replaced"].contains(&text(a, "state")));
    let source_id = v["sourceId"]
        .as_i64()
        .ok_or_else(|| ApiError::bad("请选择货源"))?;
    let source = source_list(&tx, f["bookId"].as_i64())?
        .into_iter()
        .find(|v| v["id"] == source_id)
        .ok_or_else(|| ApiError::bad("货源不属于本书"))?;
    if state != "canceled"
        && (text(&source["data"], "status") != "available" || source["data"]["dropship"] != true)
    {
        return Err(ApiError::bad("请选择已核验且支持代发的货源"));
    }
    if state == "canceled" && current.is_none() {
        return Err(ApiError::bad("没有可取消的采购记录"));
    }
    if let Some(old) = current {
        if old["state"] == "ordered" && state == "draft" {
            return Err(ApiError::bad("已采购不能退回待下单，请先核实取消采购"));
        }
        if old["data"]["source"]["id"] != source_id && old["state"] != "draft" {
            return Err(ApiError::bad(
                "已下单采购请先核实取消，再更换货源，避免重复采购",
            ));
        }
        if old["state"] == "shipped" && state != "shipped" && state != "canceled" {
            return Err(ApiError::bad("已发货采购不能退回待下单"));
        }
    }
    if ["ordered", "shipped"].contains(&state.as_str()) {
        required(&v, "purchaseNo")?;
        amount(&v, "actualCost", false)?;
        if v["confirmed"] != true {
            return Err(ApiError::bad("请核对版次、数量、收件信息和实际付款金额"));
        }
    }
    if state == "shipped" {
        required(&v, "trackingNo")?;
        required(&v, "expressCode")?;
        required(&v, "expressName")?;
    }
    if state == "canceled" {
        required(&v, "note")?;
        if v["confirmed"] != true {
            return Err(ApiError::bad("请确认淘宝采购已取消或退款处理完成"));
        }
    }
    let mut data = v.clone();
    if state == "canceled" {
        let old = current.unwrap();
        let paid = old["data"]["actualCost"].as_f64().unwrap_or(0.0);
        amount(&v, "supplierRefund", false)?;
        if v["supplierRefund"].as_f64().unwrap() > paid {
            return Err(ApiError::bad("供应商退款不能超过采购实付金额"));
        }
        for field in ["purchaseNo", "trackingNo", "expressCode", "expressName"] {
            data[field] = old["data"][field].clone();
        }
        data["actualCost"] = json!(paid);
    }
    data["source"] = source;
    data["quantity"] = f["snapshot"]["goods"]["quantity"].clone();
    let replace = current
        .map(|old| old["data"]["source"]["id"] != source_id)
        .unwrap_or(false);
    if replace {
        tx.execute(
            "UPDATE purchase_attempts SET state='replaced',updated_at=? WHERE id=?",
            params![db::now(), current.unwrap()["id"].as_i64()],
        )?;
    }
    if let Some(old) = current.filter(|_| !replace) {
        data["source"] = old["data"]["source"].clone();
        tx.execute(
            "UPDATE purchase_attempts SET state=?,data=?,updated_at=? WHERE id=?",
            params![state, data.to_string(), db::now(), old["id"].as_i64()],
        )?;
    } else {
        tx.execute("INSERT INTO purchase_attempts(fulfillment_id,state,data,created_at,updated_at) VALUES(?,?,?,?,?)",params![id,state,data.to_string(),db::now(),db::now()])?;
    }
    let next = if ["after_sale", "canceled_review"].contains(&text(&f, "state")) {
        text(&f, "state")
    } else {
        match state.as_str() {
            "ordered" => "awaiting_tracking",
            "shipped" => "ready_to_ship",
            _ => "to_purchase",
        }
    };
    tx.execute(
        "UPDATE fulfillment_orders SET state=?,updated_at=? WHERE id=?",
        params![next, db::now(), id],
    )?;
    event(&tx, "fulfillment", id, "purchase", &data)?;
    tx.commit()?;
    Ok(Json(json!({"ok":true})))
}
pub(super) async fn finance(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let f = get(&c, id)?;
    for k in ["cost", "fees", "refund"] {
        amount(&v, k, true)?
    }
    let mut d = f["data"].clone();
    d["finance"] = json!({"cost":v["cost"],"fees":v["fees"],"refund":v["refund"]});
    c.execute(
        "UPDATE fulfillment_orders SET data=?,updated_at=? WHERE id=?",
        params![d.to_string(), db::now(), id],
    )?;
    event(&c, "fulfillment", id, "finance", &d["finance"])?;
    Ok(Json(json!({"ok":true})))
}
pub(super) async fn note(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let c = lock(&s);
    let f = get(&c, id)?;
    let note = required(&v, "note")?;
    let close = v["close"] == true;
    if close && !["after_sale", "canceled_review"].contains(&text(&f, "state")) {
        return Err(ApiError::bad("只能结案已核实的售后或取消事项"));
    }
    if close {
        let mut data = f["data"].clone();
        data["closure"] = json!([
            f["snapshot"]["order_status"],
            f["snapshot"]["refund_status"],
            f["snapshot"]["refund_amount"],
            f["snapshot"]["refund_time"]
        ]);
        c.execute(
            "UPDATE fulfillment_orders SET state='closed',data=?,updated_at=? WHERE id=?",
            params![data.to_string(), db::now(), id],
        )?;
    }
    event(
        &c,
        "fulfillment",
        id,
        if close { "closed" } else { "note" },
        &json!({"note":note}),
    )?;
    Ok(Json(json!({"ok":true})))
}
