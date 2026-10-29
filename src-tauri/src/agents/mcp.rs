use super::*;
use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
pub fn authorized(s: &AppState, headers: &HeaderMap) -> bool {
    let c = lock(s);
    if db::get_setting(&c, "agentMcpEnabled") != Some(json!(true)) {
        return false;
    }
    let token = db::get_setting(&c, "agentMcpToken")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    if token.is_empty()
        || headers.get("authorization").and_then(|h| h.to_str().ok())
            != Some(format!("Bearer {token}").as_str())
    {
        return false;
    }
    origin_allowed(headers)
}
pub fn origin_allowed(headers: &HeaderMap) -> bool {
    headers.get("origin").is_none_or(|h| {
        h.to_str().ok().is_some_and(|o| {
            let Ok(u) = reqwest::Url::parse(o) else {
                return false;
            };
            ["http", "https"].contains(&u.scheme())
                && matches!(
                    u.host_str(),
                    Some("127.0.0.1" | "localhost" | "tauri.localhost")
                )
        })
    })
}
fn schema(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn string() -> Value {
    json!({"type":"string"})
}
fn tool(name: &str, description: &str, input: Value, read: bool) -> Value {
    json!({"name":name,"description":description,"inputSchema":input,"annotations":{"readOnlyHint":read,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
}
pub(super) fn tools() -> Vec<Value> {
    let common = json!({"source":string(),"externalId":string(),"sourceUrl":string()});
    let with = |extra: Value| {
        let mut v = common.clone();
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    let book_input = schema(
        with(
            json!({"title":string(),"author":string(),"publisher":string(),"isbn":string(),"edition":string(),"volume":string(),"publicationYear":string()}),
        ),
        &["source", "externalId", "sourceUrl", "title", "edition"],
    );
    let mut lookup=tool("taoshu_lookup_isbn","按书名从豆瓣建议接口查询版本候选，并提取详情页 ISBN；可指定数字 subjectId，结果不自动写入书库。带缓存及限频，受限时返回准确状态。",schema(json!({"query":string(),"subjectId":string()}),&[]),true);
    lookup["annotations"]["openWorldHint"] = json!(true);
    let mut generate=tool("taoshu_generate_draft","使用已配置的 OpenCode 或 Qoder CLI 模型分析货源、定价或生成文案/自定义文本，自动写入本地草稿；不确认主选货源、不发布。start 返回任务 ID，status 查询结果。可能消耗所选模型额度。",schema(json!({"action":{"type":"string","enum":["start","status"]},"bookIds":{"type":"array","items":{"type":"integer","minimum":1},"minItems":1,"maxItems":100},"task":{"type":"string","enum":["complete","pricing","copy","text"]},"provider":{"type":"string","enum":["opencode","qoder"]},"prompt":string(),"model":string(),"jobId":{"type":"integer","minimum":1}}),&["action"]),false);
    generate["annotations"]["idempotentHint"]=json!(false);generate["annotations"]["openWorldHint"]=json!(true);
    vec![
        tool("taoshu_context","查询淘书工坊导入约定与可用能力，不包含订单或凭据。",schema(json!({}),&[]),true),
        tool("taoshu_search_books","按书名、ISBN、作者查询已有版本，仅返回公开书目字段。",schema(json!({"query":string()}),&["query"]),true),
        tool("taoshu_import_book","导入有来源证据的图书版本。严格核对 ISBN、版次、册次，保留既有图书，不覆盖报价、库存或销售数据。",book_input.clone(),false),
        tool("taoshu_import_books","批量入库 1–200 本有来源依据的图书。逐条事务、版本去重及幂等，返回每条 bookId 或失败原因；一条失败不回滚其他图书。",schema(json!({"books":{"type":"array","minItems":1,"maxItems":200,"items":book_input}}),&["books"]),false),
        lookup,
        generate,
        tool("taoshu_import_school_book","将已有图书版本关联到学校、年份、专业及官方用书引用。",schema(with(json!({"bookId":{"type":"integer","minimum":1},"school":string(),"year":{"type":"integer","minimum":2000,"maximum":2200},"major":string(),"college":string(),"majorCode":string(),"subject":string(),"stage":string(),"raw":string(),"edition":string(),"volume":string()})),&["source","externalId","sourceUrl","bookId","school","year","major","raw","edition"]),false),
        tool("taoshu_import_source","向指定图书导入货源候选；不覆盖已核验报价，不推断运费与代发。",schema(with(json!({"bookId":{"type":"integer","minimum":1},"url":string(),"title":string(),"shop":string(),"sku":string(),"condition":string(),"price":{"type":["number","null"],"minimum":0},"freight":{"type":["number","null"],"minimum":0}})),&["source","externalId","sourceUrl","bookId","url"]),false),
        tool("taoshu_import_image","向指定图书上传实际 PNG/JPEG/WebP 图片的 Base64，最多 10MB。不读取本机文件，不代抓远程网址。",schema(with(json!({"bookId":{"type":"integer","minimum":1},"base64":string(),"mimeType":{"type":"string","enum":["image/png","image/jpeg","image/webp"]}})),&["source","externalId","sourceUrl","bookId","base64","mimeType"]),false),
    ]
}
fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub(super) fn dispatch(s: &AppState, v: Value) -> Value {
    let id = v["id"].clone();
    if v["jsonrpc"] != "2.0" || !v["method"].is_string() || !(id.is_string() || id.is_number()) {
        return rpc_error(Value::Null, -32600, "Invalid JSON-RPC request");
    }
    let method = v["method"].as_str().unwrap();
    let result = match method {
        "initialize" => {
            let requested = v["params"]["protocolVersion"].as_str().unwrap_or("");
            let protocol = if ["2025-03-26", "2025-06-18", "2025-11-25"].contains(&requested) {
                requested
            } else {
                "2025-11-25"
            };
            json!({"protocolVersion":protocol,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"taoshu","version":env!("CARGO_PKG_VERSION")},"instructions":"先调用 taoshu_context。导入保留 source、externalId 和来源证据；不提供交易、发货或凭据工具。"})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":tools()}),
        "tools/call" => {
            let name = v["params"]["name"].as_str().unwrap_or("");
            let args = &v["params"]["arguments"];
            let Some(def) = tools().into_iter().find(|t| t["name"] == name) else {
                return rpc_error(id, -32602, "Unknown tool");
            };
            let args = if args.is_null() {
                json!({})
            } else {
                args.clone()
            };
            let Some(fields) = args.as_object() else {
                return rpc_error(id, -32602, "arguments must be an object");
            };
            let props = def["inputSchema"]["properties"].as_object().unwrap();
            if fields.keys().any(|key| !props.contains_key(key)) {
                return rpc_error(id, -32602, "Unsupported argument");
            }
            for required in def["inputSchema"]["required"].as_array().unwrap() {
                if !fields.contains_key(required.as_str().unwrap()) {
                    return rpc_error(id, -32602, "Missing required argument");
                }
            }
            for (key, value) in fields {
                if key != "base64" && value.as_str().is_some_and(|v| v.len() > if key=="prompt"{12000}else{3000}) {
                    return rpc_error(id, -32602, "Argument is too long");
                }
                let def = &props[key];
                let kinds = if let Some(kind) = def["type"].as_str() {
                    vec![kind]
                } else {
                    def["type"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Value::as_str)
                        .collect()
                };
                let valid = kinds.iter().any(|kind| match *kind {
                    "string" => value.is_string(),
                    "integer" => value.is_i64(),
                    "number" => value.is_number(),
                    "null" => value.is_null(),
                    "array" => value.is_array(),
                    _ => false,
                });
                if !valid
                    || def["enum"]
                        .as_array()
                        .is_some_and(|values| !values.contains(value))
                {
                    return rpc_error(id, -32602, "Invalid argument type or value");
                }
            }
            match imports::execute(s, name, &args) {
                Ok(data) => {
                    let failed = matches!(
                        data["state"].as_str(),
                        Some("blocked" | "failed" | "rate_limited")
                    );
                    json!({"content":[{"type":"text","text":data.to_string()}],"structuredContent":data,"isError":failed})
                }
                Err(message) => json!({"content":[{"type":"text","text":message}],"isError":true}),
            }
        }
        _ => return rpc_error(id, -32601, "Method not found"),
    };
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
pub(super) async fn post(State(s): State<AppState>, headers: HeaderMap, bytes: Bytes) -> Response {
    if bytes.len() > 16 * 1024 * 1024 {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(rpc_error(Value::Null, -32600, "Request too large")),
        )
            .into_response();
    }
    let v: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return Json(rpc_error(Value::Null, -32700, "Parse error")).into_response(),
    };
    if let Some(protocol) = headers.get("mcp-protocol-version") {
        if !["2025-03-26", "2025-06-18", "2025-11-25"].contains(&protocol.to_str().unwrap_or("")) {
            return (
                StatusCode::BAD_REQUEST,
                Json(rpc_error(
                    v["id"].clone(),
                    -32600,
                    "Unsupported MCP protocol version",
                )),
            )
                .into_response();
        }
    }
    if v["jsonrpc"] == "2.0"
        && v.get("id").is_none()
        && v["method"]
            .as_str()
            .is_some_and(|m| m.starts_with("notifications/"))
    {
        return StatusCode::ACCEPTED.into_response();
    }
    if v["jsonrpc"] == "2.0"
        && v.get("method").is_none()
        && (v.get("result").is_some() || v.get("error").is_some())
    {
        return StatusCode::ACCEPTED.into_response();
    }
    match tokio::task::spawn_blocking(move || dispatch(&s, v)).await {
        Ok(result) => Json(result).into_response(),
        Err(_) => Json(rpc_error(Value::Null, -32603, "Internal error")).into_response(),
    }
}
pub(super) async fn no_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}
