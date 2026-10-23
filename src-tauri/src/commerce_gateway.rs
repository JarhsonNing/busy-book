use super::*;
use crate::commerce as business;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/commerce/sync-orders", post(sync_orders))
        .route("/api/commerce/channels/{id}/remote", post(remote_channel))
        .route("/api/commerce/channels/{id}/map", post(map_channel))
        .route(
            "/api/commerce/fulfillments/{id}/refresh",
            post(refresh_order),
        )
        .route("/api/commerce/fulfillments/{id}/ship", post(ship))
}

fn channel(state: &AppState, id: i64, key: &str) -> Result<Value, ApiError> {
    let c = lock(state);
    business::rows(&c,"SELECT json_object('id',id,'bookId',book_id,'shop',shop,'productId',product_id,'data',json(data)) FROM channel_products WHERE id=? AND app_key=?",params![id,key])?.pop().ok_or_else(||ApiError::bad("渠道商品不存在或属于其他应用"))
}
fn fulfillment(state: &AppState, id: i64, key: &str) -> Result<Value, ApiError> {
    business::rows(&lock(state),"SELECT json_object('id',id,'orderNo',order_no,'bookId',book_id,'kind',kind,'state',state,'snapshot',json(snapshot),'data',json(data)) FROM fulfillment_orders WHERE id=? AND app_key=?",params![id,key])?.pop().ok_or_else(||ApiError::bad("订单不存在或属于其他应用"))
}

pub(super) async fn inventory(
    state: &AppState,
    cfg: &ConfigInput,
    product_id: &str,
    sku_id: &str,
) -> Value {
    let Ok(pid) = product_id.parse::<u64>() else {
        return json!({"stock":null,"skuStock":null,"error":"缺少有效管家商品 ID","checkedAt":db::now()});
    };
    let data = state
        .xianyu
        .call(cfg, "/api/open/product/detail", &json!({"product_id":pid}))
        .await;
    let mut result = json!({"stock":null,"skuStock":null,"skuItems":[],"error":"","skuError":"","checkedAt":db::now()});
    match data {
        Err(e) => {
            result["error"] = json!(e.message);
            return result;
        }
        Ok(data) => {
            result["stock"] = data["stock"].clone();
            if data["stock"].as_u64().is_none() {
                result["error"] = json!("远端未返回有效商品总库存");
            }
            result["product"] = data.clone();
            let multi = data["spec_type"].as_i64() == Some(2)
                || data["sku_items"]
                    .as_array()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false)
                || (!sku_id.is_empty() && sku_id != "0");
            if !multi && data["spec_type"].as_i64() == Some(1) {
                if sku_id == "0" {
                    result["skuStock"] = data["stock"].clone();
                }
                result["singleSpec"] = json!(true);
                return result;
            }
        }
    }
    match state
        .xianyu
        .call(
            cfg,
            "/api/open/product/sku/list",
            &json!({"product_id":[pid]}),
        )
        .await
    {
        Err(e) => result["skuError"] = json!(e.message),
        Ok(data) => {
            let list = data["list"]
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|p| business::remote_id(&p["product_id"]) == product_id)
                })
                .and_then(|p| p["sku_items"].as_array())
                .cloned()
                .unwrap_or_default();
            if let Some(sku) = list
                .iter()
                .find(|v| business::remote_id(&v["sku_id"]) == sku_id)
            {
                result["skuStock"] = sku["stock"].clone();
            } else if !sku_id.is_empty() {
                result["skuError"] = json!("未查到本单规格库存，商品总库存不能代替规格库存");
            }
            result["skuItems"] = json!(list);
        }
    }
    result
}
async fn remote_channel(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let cfg = config(&s)?;
    let ch = channel(&s, id, &cfg.app_key)?;
    let pid = if business::text(&v, "productId").is_empty() {
        business::text(&ch, "productId")
    } else {
        business::text(&v, "productId")
    };
    let mut result = inventory(&s, &cfg, pid, "").await;
    if !result["error"].as_str().unwrap_or("").is_empty() {
        return Err(ApiError::bad(result["error"].as_str().unwrap()));
    }
    // Even if the product detail omits spec_type, request specs explicitly for mapping inspection.
    if result["singleSpec"] == true {
        let mut payload = json!({"product_id":pid});
        integer_id(&mut payload, "product_id")?;
        if let Ok(data) = s
            .xianyu
            .call(
                &cfg,
                "/api/open/product/sku/list",
                &json!({"product_id":[payload["product_id"]]}),
            )
            .await
        {
            result["skuItems"] = data["list"]
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|p| business::remote_id(&p["product_id"]) == pid)
                })
                .map(|p| p["sku_items"].clone())
                .unwrap_or(json!([]));
        }
    }
    browser_ids(&mut result);
    Ok(Json(result))
}
async fn map_channel(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let cfg = config(&s)?;
    let ch = channel(&s, id, &cfg.app_key)?;
    let pid = business::text(&v, "productId");
    let mut body = json!({"productId":pid});
    let Json(remote) = remote_channel(State(s.clone()), Path(id), Json(body.take())).await?;
    let shops = remote["product"]["publish_shop"]
        .as_array()
        .ok_or_else(|| ApiError::bad("远端缺少店铺信息"))?;
    let shop = shops
        .iter()
        .find(|p| p["user_name"] == ch["shop"])
        .ok_or_else(|| ApiError::bad("该商品不属于选择的店铺"))?;
    let physical = business::text(&v, "physicalSkuId");
    let digital = business::text(&v, "digitalSkuId");
    let mode = business::text(&ch["data"], "mode");
    let skus = remote["skuItems"].as_array().cloned().unwrap_or_default();
    let valid = |id: &str| {
        skus.iter()
            .any(|sku| business::remote_id(&sku["sku_id"]) == id)
    };
    if !(valid(physical)
        || (mode == "physical"
            && physical == "0"
            && remote["singleSpec"] == true
            && skus.is_empty()))
    {
        return Err(ApiError::bad("实体规格 ID 不属于远端商品"));
    }
    if mode == "mixed" && (!valid(digital) || digital == physical || digital == "0") {
        return Err(ApiError::bad("电子和实体须选择不同的有效远端规格"));
    }
    if v["confirmed"] != true {
        return Err(ApiError::bad("请核对远端规格文字及对应售卖形态"));
    }
    let c = lock(&s);
    let old_product = business::text(&ch, "productId");
    if !old_product.is_empty() && old_product != pid {
        return Err(ApiError::bad("已有商品绑定，不能覆盖为另一个商品"));
    }
    let count: i64 = c.query_row(
        "SELECT count(*) FROM fulfillment_orders WHERE channel_id=? AND kind<>'unknown'",
        [id],
        |r| r.get(0),
    )?;
    if count > 0
        && ((!business::text(&ch["data"], "physicalSkuId").is_empty()
            && business::text(&ch["data"], "physicalSkuId") != physical)
            || (!business::text(&ch["data"], "digitalSkuId").is_empty()
                && business::text(&ch["data"], "digitalSkuId") != digital))
    {
        return Err(ApiError::bad("该映射已有履约订单，不能直接更换规格身份"));
    }
    let mut data = ch["data"].clone();
    data["physicalSkuId"] = json!(physical);
    data["digitalSkuId"] = json!(digital);
    data["verifiedMapping"] = json!(true);
    data["itemId"] = shop["item_id"].clone();
    data["state"] = json!(if remote["product"]["product_status"] == 22 {
        "published"
    } else {
        "created"
    });
    c.execute(
        "UPDATE channel_products SET product_id=?,data=?,updated_at=? WHERE id=?",
        params![pid, data.to_string(), db::now(), id],
    )?;
    business::event(
        &c,
        "channel",
        id,
        "mapped",
        &json!({"productId":pid,"physicalSkuId":physical,"digitalSkuId":digital}),
    )?;
    Ok(Json(json!({"ok":true})))
}

async fn sync_orders(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let _guard = s
        .xianyu
        .operation
        .try_lock()
        .map_err(|_| ApiError::bad("已有同步或远端操作进行中"))?;
    let cfg = config(&s)?;
    let key = format!("commerceOrderCursor:{}", cfg.app_key);
    let end = chrono::Utc::now().timestamp();
    let days = v["days"].as_i64().unwrap_or(30).clamp(1, 180);
    let saved = db::get_setting(&lock(&s), &key).and_then(|v| v.as_i64());
    let start = if v["full"] == true {
        end - days * 86400
    } else {
        saved
            .map(|v| v - 300)
            .unwrap_or(end - days * 86400)
            .max(end - 180 * 86400)
    };
    let mut count = 0;
    let mut complete = false;
    for page in 1..=100 {
        let data = s
            .xianyu
            .call(
                &cfg,
                "/api/open/order/list",
                &json!({"page_no":page,"page_size":100,"update_time":[start,end]}),
            )
            .await
            .map_err(|e| ApiError::bad(e.message))?;
        let list = data["list"]
            .as_array()
            .ok_or_else(|| ApiError::bad("订单列表响应缺少 list"))?;
        for order in list {
            let mut order = order.clone();
            orders::enrich(&s, &cfg.app_key, &mut order)?;
            count += 1;
        }
        if list.len() < 100
            || data["count"]
                .as_i64()
                .map(|n| page * 100 >= n)
                .unwrap_or(false)
        {
            complete = true;
            break;
        }
    }
    if !complete {
        return Err(ApiError::bad(
            "同步达到一万条上限，游标未推进；请缩短时间范围后重试",
        ));
    }
    db::set_setting(&lock(&s), &key, &json!(end))?;
    let pending=business::rows(&lock(&s),"SELECT json_object('orderNo',order_no) FROM fulfillment_orders WHERE app_key=? AND kind='digital' AND state='delivery_pending' ORDER BY updated_at LIMIT 100",[&cfg.app_key])?;
    let mut delivery_errors = Vec::new();
    for f in pending {
        let no = business::text(&f, "orderNo");
        match s
            .xianyu
            .call(&cfg, "/api/open/order/kam/list", &json!({"order_no":no}))
            .await
        {
            Ok(cards) => business::record_delivery(&lock(&s), &cfg.app_key, no, &cards)?,
            Err(e) => delivery_errors.push(json!({"orderNo":no,"error":e.message})),
        }
    }
    Ok(Json(
        json!({"count":count,"from":start,"to":end,"deliveryErrors":delivery_errors}),
    ))
}
async fn refresh_order(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let cfg = config(&s)?;
    let f = fulfillment(&s, id, &cfg.app_key)?;
    let mut order = s
        .xianyu
        .call(
            &cfg,
            "/api/open/order/detail",
            &json!({"order_no":f["orderNo"]}),
        )
        .await
        .map_err(|e| ApiError::bad(e.message))?;
    if order["order_no"] != f["orderNo"] {
        return Err(ApiError::bad("返回的订单号不匹配"));
    }
    orders::enrich(&s, &cfg.app_key, &mut order)?;
    let current = fulfillment(&s, id, &cfg.app_key)?;
    let mut delivery_error = Value::Null;
    if current["kind"] == "digital" {
        match s
            .xianyu
            .call(
                &cfg,
                "/api/open/order/kam/list",
                &json!({"order_no":f["orderNo"]}),
            )
            .await
        {
            Ok(cards) => business::record_delivery(
                &lock(&s),
                &cfg.app_key,
                business::text(&f, "orderNo"),
                &cards,
            )?,
            Err(e) => delivery_error = json!(e.message),
        }
    }
    let mut result = inventory(
        &s,
        &cfg,
        &business::remote_id(&order["goods"]["product_id"]),
        &business::remote_id(&order["goods"]["sku_id"]),
    )
    .await;
    result["order"] = order;
    result["deliveryError"] = delivery_error;
    browser_ids(&mut result);
    Ok(Json(result))
}
async fn ship(State(s): State<AppState>, Path(id): Path<i64>) -> Result<Json<Value>, ApiError> {
    let _guard = s
        .xianyu
        .operation
        .try_lock()
        .map_err(|_| ApiError::bad("已有远端操作进行中"))?;
    let cfg = config(&s)?;
    let f = fulfillment(&s, id, &cfg.app_key)?;
    if f["kind"] != "physical" {
        return Err(ApiError::bad("只有实体订单可回传物流"));
    }
    let mut order = s
        .xianyu
        .call(
            &cfg,
            "/api/open/order/detail",
            &json!({"order_no":f["orderNo"]}),
        )
        .await
        .map_err(|e| ApiError::bad(e.message))?;
    if order["order_no"] != f["orderNo"] {
        return Err(ApiError::bad("返回的订单号不匹配"));
    }
    orders::enrich(&s, &cfg.app_key, &mut order)?;
    if [21, 22].contains(&order["order_status"].as_i64().unwrap_or(0)) {
        return Ok(Json(json!({"status":"already_shipped"})));
    }
    if order["order_status"] != 12
        || business::refund_blocks(order["refund_status"].as_i64().unwrap_or(0))
    {
        return Err(ApiError::bad("订单不处于可发货状态"));
    }
    if f["state"] == "shipping_unknown" || f["data"]["shippingUncertain"] == true {
        return Err(ApiError::bad(
            "上次发货结果未知，请核查闲管家后处理，禁止盲目重发",
        ));
    }
    let attempt=business::rows(&lock(&s),"SELECT data FROM purchase_attempts WHERE fulfillment_id=? AND state='shipped' ORDER BY id DESC LIMIT 1",[id])?.pop().ok_or_else(||ApiError::bad("先填写已发货采购的物流信息"))?;
    let body = json!({"order_no":f["orderNo"],"waybill_no":attempt["trackingNo"],"express_code":attempt["expressCode"],"express_name":attempt["expressName"]});
    let (_, body) = operation_body("ship", body)?;
    // Persist uncertain state before sending, including process termination between send and receive.
    lock(&s).execute(
        "UPDATE fulfillment_orders SET state='shipping_unknown',data=json_set(data,'$.shippingUncertain',json('true')),updated_at=? WHERE id=?",
        params![db::now(), id],
    )?;
    match s.xianyu.call(&cfg, "/api/open/order/ship", &body).await {
        Ok(_) => {
            business::event(&lock(&s), "fulfillment", id, "shipping_accepted", &body)?;
            Ok(Json(
                json!({"status":"accepted","message":"发货已受理，请刷新订单核查远端状态"}),
            ))
        }
        Err(e) => {
            if e.rejected {
                lock(&s).execute(
                    "UPDATE fulfillment_orders SET state='ready_to_ship',data=json_set(data,'$.shippingUncertain',json('false')) WHERE id=?",
                    [id],
                )?;
            }
            business::event(
                &lock(&s),
                "fulfillment",
                id,
                "shipping_error",
                &json!({"error":e.message}),
            )?;
            Err(ApiError::bad(e.message))
        }
    }
}
