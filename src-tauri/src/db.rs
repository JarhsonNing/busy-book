use chrono::{SecondsFormat, Utc};
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension, Row};
use serde_json::{json, Value as Json};

use crate::models::*;

pub const BOOK_SELECT: &str = "
  SELECT b.*,
    (SELECT COUNT(*) FROM images i WHERE i.book_id = b.id) AS image_count,
    (SELECT i.id FROM images i WHERE i.book_id = b.id
       ORDER BY i.is_primary DESC, i.sort ASC, i.id ASC LIMIT 1) AS cover_id,
    (SELECT CASE WHEN i.processed_file <> '' THEN 1 ELSE 0 END FROM images i WHERE i.book_id = b.id
       ORDER BY i.is_primary DESC, i.sort ASC, i.id ASC LIMIT 1) AS cover_processed,
    (SELECT l.id FROM listings l WHERE l.book_id = b.id ORDER BY l.id DESC LIMIT 1) AS listing_id,
    (SELECT l.status FROM listings l WHERE l.book_id = b.id ORDER BY l.id DESC LIMIT 1) AS listing_status
  FROM books b";

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS captures (
          id             INTEGER PRIMARY KEY AUTOINCREMENT,
          item_id        TEXT UNIQUE,
          url            TEXT DEFAULT '',
          title          TEXT DEFAULT '',
          price          REAL,
          original_price REAL,
          shop           TEXT DEFAULT '',
          sales          TEXT DEFAULT '',
          main_image     TEXT DEFAULT '',
          images         TEXT DEFAULT '[]',
          specs          TEXT DEFAULT '{}',
          raw            TEXT DEFAULT '{}',
          created_at     TEXT NOT NULL,
          updated_at     TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS books (
          id          INTEGER PRIMARY KEY AUTOINCREMENT,
          capture_id  INTEGER REFERENCES captures(id) ON DELETE SET NULL,
          item_id     TEXT,
          title       TEXT NOT NULL DEFAULT '',
          author      TEXT DEFAULT '',
          publisher   TEXT DEFAULT '',
          isbn        TEXT DEFAULT '',
          edition     TEXT DEFAULT '',
          condition   TEXT DEFAULT '九成新',
          cost_price  REAL,
          list_price  REAL,
          sell_price  REAL,
          stock       INTEGER NOT NULL DEFAULT 1,
          status      TEXT NOT NULL DEFAULT 'draft',
          note        TEXT DEFAULT '',
          created_at  TEXT NOT NULL,
          updated_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS images (
          id             INTEGER PRIMARY KEY AUTOINCREMENT,
          book_id        INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
          source         TEXT NOT NULL DEFAULT 'kongfz',
          origin_url     TEXT DEFAULT '',
          origin_file    TEXT DEFAULT '',
          processed_file TEXT DEFAULT '',
          width          INTEGER,
          height         INTEGER,
          is_primary     INTEGER NOT NULL DEFAULT 0,
          sort           INTEGER NOT NULL DEFAULT 0,
          created_at     TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS listings (
          id          INTEGER PRIMARY KEY AUTOINCREMENT,
          book_id     INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
          platform    TEXT NOT NULL DEFAULT 'xianyu',
          title       TEXT DEFAULT '',
          description TEXT DEFAULT '',
          price       REAL,
          status      TEXT NOT NULL DEFAULT 'draft',
          url         TEXT DEFAULT '',
          error       TEXT DEFAULT '',
          created_at  TEXT NOT NULL,
          updated_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS xianyu_tasks (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          book_id INTEGER REFERENCES books(id) ON DELETE SET NULL,
          app_key TEXT NOT NULL,
          user_name TEXT NOT NULL,
          title TEXT NOT NULL,
          product_id TEXT NOT NULL DEFAULT '',
          item_id TEXT NOT NULL DEFAULT '',
          status TEXT NOT NULL DEFAULT 'preparing',
          image_mode TEXT NOT NULL,
          images_verified INTEGER NOT NULL DEFAULT 0,
          error TEXT NOT NULL DEFAULT '',
          attempts TEXT NOT NULL DEFAULT '[]',
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_xianyu_book ON xianyu_tasks(book_id, app_key, user_name);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_xianyu_active ON xianyu_tasks(book_id, app_key, user_name)
          WHERE product_id <> '' OR status IN ('preparing', 'unknown');

        CREATE TABLE IF NOT EXISTS xianyu_orders (
          app_key TEXT NOT NULL,
          order_no TEXT NOT NULL,
          book_id INTEGER REFERENCES books(id) ON DELETE SET NULL,
          manual INTEGER NOT NULL DEFAULT 0,
          summary TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          PRIMARY KEY(app_key, order_no)
        );
        CREATE INDEX IF NOT EXISTS idx_xianyu_orders_book ON xianyu_orders(book_id, app_key);

        CREATE TABLE IF NOT EXISTS settings (
          key   TEXT PRIMARY KEY,
          value TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_books_status ON books(status);
        CREATE INDEX IF NOT EXISTS idx_books_item ON books(item_id);
        CREATE INDEX IF NOT EXISTS idx_images_book ON images(book_id);
        CREATE INDEX IF NOT EXISTS idx_listings_book ON listings(book_id);
        ",
    )?;
    crate::commerce::schema::init(conn)
}

/* --------------------------------- 设置 --------------------------------- */

pub fn get_setting(conn: &Connection, key: &str) -> Option<Json> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?", [key], |row| {
            row.get(0)
        })
        .optional()
        .ok()
        .flatten();
    raw.and_then(|text| serde_json::from_str(&text).ok())
}

pub fn set_setting(conn: &Connection, key: &str, value: &Json) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value.to_string()],
    )?;
    Ok(())
}

pub fn settings(conn: &Connection) -> Settings {
    let mut value = Settings::default();
    if let Some(Json::Number(number)) = get_setting(conn, "profitRate") {
        if let Some(rate) = number.as_f64() {
            if (0.0..=100.0).contains(&rate) {
                value.profit_rate = rate;
            }
        }
    }
    if let Some(Json::Number(number)) = get_setting(conn, "minProfit") {
        if let Some(profit) = number.as_f64() {
            if (0.0..=10000.0).contains(&profit) {
                value.min_profit = profit;
            }
        }
    }
    for (key, field) in [
        ("defaultCondition", &mut value.default_condition),
        ("titleSuffix", &mut value.title_suffix),
        ("descFooter", &mut value.desc_footer),
        ("publishUrl", &mut value.publish_url),
        ("kongfzUrl", &mut value.kongfz_url),
    ] {
        if let Some(Json::String(text)) = get_setting(conn, key) {
            if !text.trim().is_empty() {
                *field = text;
            }
        }
    }
    if let Some(Json::Bool(flag)) = get_setting(conn, "autoSubmit") {
        value.auto_submit = flag;
    }
    value
}

pub fn save_settings(conn: &Connection, patch: &Json) -> rusqlite::Result<Settings> {
    if let Some(object) = patch.as_object() {
        for (key, value) in object {
            match key.as_str() {
                "profitRate" => {
                    if let Some(number) = value.as_f64() {
                        let clamped = number.clamp(0.0, 100.0);
                        set_setting(conn, key, &json!(clamped))?;
                    }
                }
                "minProfit" => {
                    if let Some(number) = value.as_f64() {
                        let clamped = number.clamp(0.0, 10000.0);
                        set_setting(conn, key, &json!(clamped))?;
                    }
                }
                // Accept the old 1.35-style markup from stale clients and convert it.
                "markup" => {
                    if let Some(number) = value.as_f64() {
                        let rate = ((number - 1.0) * 100.0).clamp(0.0, 100.0);
                        set_setting(conn, "profitRate", &json!(rate))?;
                    }
                }
                "defaultCondition" | "titleSuffix" | "descFooter" | "publishUrl" | "kongfzUrl" => {
                    if let Some(text) = value.as_str() {
                        set_setting(conn, key, &json!(text.trim()))?;
                    }
                }
                "autoSubmit" => {
                    if let Some(flag) = value.as_bool() {
                        set_setting(conn, key, &json!(flag))?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(settings(conn))
}

/* -------------------------------- 行映射 -------------------------------- */

fn parse_json<T: serde::de::DeserializeOwned + Default>(text: &str) -> T {
    serde_json::from_str(text).unwrap_or_default()
}

fn capture_from_row(row: &Row<'_>) -> rusqlite::Result<Capture> {
    Ok(Capture {
        id: row.get("id")?,
        item_id: row.get::<_, Option<String>>("item_id")?.unwrap_or_default(),
        url: row.get::<_, Option<String>>("url")?.unwrap_or_default(),
        title: row.get::<_, Option<String>>("title")?.unwrap_or_default(),
        price: row.get("price")?,
        original_price: row.get("original_price")?,
        shop: row.get::<_, Option<String>>("shop")?.unwrap_or_default(),
        sales: row.get::<_, Option<String>>("sales")?.unwrap_or_default(),
        main_image: row
            .get::<_, Option<String>>("main_image")?
            .unwrap_or_default(),
        images: parse_json::<Vec<String>>(
            &row.get::<_, Option<String>>("images")?.unwrap_or_default(),
        ),
        specs: parse_json::<Json>(&row.get::<_, Option<String>>("specs")?.unwrap_or_default()),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn book_from_row(row: &Row<'_>) -> rusqlite::Result<Book> {
    let cover_id: Option<i64> = row.get("cover_id")?;
    let cover_processed: Option<i64> = row.get("cover_processed")?;
    let cover_url = cover_id
        .map(|id| {
            format!(
                "/api/images/{}/file{}",
                id,
                if cover_processed.unwrap_or(0) == 1 {
                    "?variant=processed"
                } else {
                    ""
                }
            )
        })
        .unwrap_or_default();

    Ok(Book {
        id: row.get("id")?,
        capture_id: row.get("capture_id")?,
        item_id: row.get::<_, Option<String>>("item_id")?.unwrap_or_default(),
        title: row.get::<_, Option<String>>("title")?.unwrap_or_default(),
        author: row.get::<_, Option<String>>("author")?.unwrap_or_default(),
        publisher: row
            .get::<_, Option<String>>("publisher")?
            .unwrap_or_default(),
        isbn: row.get::<_, Option<String>>("isbn")?.unwrap_or_default(),
        edition: row.get::<_, Option<String>>("edition")?.unwrap_or_default(),
        condition: row
            .get::<_, Option<String>>("condition")?
            .unwrap_or_default(),
        cost_price: row.get("cost_price")?,
        list_price: row.get("list_price")?,
        sell_price: row.get("sell_price")?,
        stock: row.get::<_, Option<i64>>("stock")?.unwrap_or(1),
        status: row
            .get::<_, Option<String>>("status")?
            .unwrap_or_else(|| "draft".into()),
        note: row.get::<_, Option<String>>("note")?.unwrap_or_default(),
        image_count: row.get::<_, Option<i64>>("image_count")?.unwrap_or(0),
        cover_url,
        listing_id: row.get("listing_id")?,
        listing_status: row.get("listing_status")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn image_row_from_row(row: &Row<'_>) -> rusqlite::Result<ImageRow> {
    Ok(ImageRow {
        id: row.get("id")?,
        book_id: row.get("book_id")?,
        source: row.get::<_, Option<String>>("source")?.unwrap_or_default(),
        origin_url: row
            .get::<_, Option<String>>("origin_url")?
            .unwrap_or_default(),
        origin_file: row
            .get::<_, Option<String>>("origin_file")?
            .unwrap_or_default(),
        processed_file: row
            .get::<_, Option<String>>("processed_file")?
            .unwrap_or_default(),
        width: row.get("width")?,
        height: row.get("height")?,
        is_primary: row.get::<_, Option<i64>>("is_primary")?.unwrap_or(0),
        sort: row.get::<_, Option<i64>>("sort")?.unwrap_or(0),
        created_at: row.get("created_at")?,
    })
}

fn image_item(row: ImageRow) -> ImageItem {
    let has_origin = !row.origin_file.is_empty();
    let processed = !row.processed_file.is_empty();
    ImageItem {
        id: row.id,
        book_id: row.book_id,
        source: row.source,
        origin_url: row.origin_url,
        has_origin,
        processed,
        width: row.width,
        height: row.height,
        is_primary: row.is_primary == 1,
        sort: row.sort,
        url: format!("/api/images/{}/file", row.id),
        processed_url: if processed {
            format!("/api/images/{}/file?variant=processed", row.id)
        } else {
            String::new()
        },
        created_at: row.created_at,
    }
}

fn listing_from_row(row: &Row<'_>) -> rusqlite::Result<Listing> {
    Ok(Listing {
        id: row.get("id")?,
        book_id: row.get("book_id")?,
        platform: row
            .get::<_, Option<String>>("platform")?
            .unwrap_or_default(),
        title: row.get::<_, Option<String>>("title")?.unwrap_or_default(),
        description: row
            .get::<_, Option<String>>("description")?
            .unwrap_or_default(),
        price: row.get("price")?,
        status: row
            .get::<_, Option<String>>("status")?
            .unwrap_or_else(|| "draft".into()),
        url: row.get::<_, Option<String>>("url")?.unwrap_or_default(),
        error: row.get::<_, Option<String>>("error")?.unwrap_or_default(),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/* -------------------------------- 采集入库 ------------------------------- */

fn json_number(value: &Option<Json>) -> Option<f64> {
    let raw = value.as_ref()?;
    if let Some(number) = raw.as_f64() {
        return Some(round2(number));
    }
    let text = raw.as_str()?.replace([',', '¥', '￥'], "");
    text.trim().parse::<f64>().ok().map(round2)
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn text_of(value: &Option<String>) -> String {
    value
        .as_deref()
        .unwrap_or("")
        .trim()
        .chars()
        .take(500)
        .collect()
}

pub fn upsert_capture(
    conn: &Connection,
    input: &CaptureInput,
    profit_rate: f64,
    min_profit: f64,
    default_condition: &str,
) -> rusqlite::Result<(Capture, Option<Book>, bool)> {
    let timestamp = now();
    let item_id = input.item_id.as_deref().unwrap_or("").trim().to_string();
    let price = json_number(&input.price);
    let original_price = json_number(&input.original_price);
    let images = input.images.clone().unwrap_or_default();
    let main_image = input
        .main_image
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| images.first().cloned())
        .unwrap_or_default();
    let specs = input.specs.clone().unwrap_or_else(|| json!({}));
    let raw = input.raw.clone().unwrap_or_else(|| json!({}));

    let existing: Option<(i64, String, String, String, String, String, Option<f64>)> =
        if item_id.is_empty() {
            None
        } else {
            conn.query_row(
            "SELECT id, url, title, shop, sales, main_image, price FROM captures WHERE item_id = ?",
            [&item_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?
        };

    let capture_id = match existing {
        Some((id, old_url, old_title, old_shop, old_sales, old_main, old_price)) => {
            conn.execute(
                "UPDATE captures SET url=?, title=?, price=?, original_price=?, shop=?, sales=?,
                    main_image=?, images=?, specs=?, raw=?, updated_at=? WHERE id=?",
                params![
                    if input.url.as_deref().unwrap_or("").is_empty() {
                        old_url
                    } else {
                        text_of(&input.url)
                    },
                    if text_of(&input.title).is_empty() {
                        old_title
                    } else {
                        text_of(&input.title)
                    },
                    price.or(old_price),
                    original_price,
                    if text_of(&input.shop).is_empty() {
                        old_shop
                    } else {
                        text_of(&input.shop)
                    },
                    if text_of(&input.sales).is_empty() {
                        old_sales
                    } else {
                        text_of(&input.sales)
                    },
                    if main_image.is_empty() {
                        old_main
                    } else {
                        main_image.clone()
                    },
                    serde_json::to_string(&images).unwrap_or_else(|_| "[]".into()),
                    serde_json::to_string(&specs).unwrap_or_else(|_| "{}".into()),
                    serde_json::to_string(&raw).unwrap_or_else(|_| "{}".into()),
                    timestamp,
                    id
                ],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO captures(item_id, url, title, price, original_price, shop, sales,
                    main_image, images, specs, raw, created_at, updated_at)
                 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    if item_id.is_empty() {
                        None
                    } else {
                        Some(&item_id)
                    },
                    text_of(&input.url),
                    text_of(&input.title),
                    price,
                    original_price,
                    text_of(&input.shop),
                    text_of(&input.sales),
                    main_image,
                    serde_json::to_string(&images).unwrap_or_else(|_| "[]".into()),
                    serde_json::to_string(&specs).unwrap_or_else(|_| "{}".into()),
                    serde_json::to_string(&raw).unwrap_or_else(|_| "{}".into()),
                    timestamp,
                    timestamp
                ],
            )?;
            conn.last_insert_rowid()
        }
    };

    let capture = conn.query_row(
        "SELECT * FROM captures WHERE id = ?",
        [capture_id],
        capture_from_row,
    )?;

    let mut book_id: Option<i64> = if item_id.is_empty() {
        None
    } else {
        conn.query_row(
            "SELECT id FROM books WHERE item_id = ? ORDER BY id DESC LIMIT 1",
            [&item_id],
            |row| row.get(0),
        )
        .optional()?
    };

    let mut created = false;
    if book_id.is_none() && !input.as_source {
        let title = text_of(&input.title);
        if !title.is_empty() || !item_id.is_empty() {
            let spec_text = |keys: &[&str]| -> String {
                keys.iter()
                    .find_map(|key| specs.get(*key).and_then(|value| value.as_str()))
                    .unwrap_or("")
                    .trim()
                    .chars()
                    .take(120)
                    .collect()
            };
            let isbn = spec_text(&["isbn", "ISBN", "I S B N"]).replace('-', "");
            let author = spec_text(&["作者", "作 者", "author"]);
            let publisher = spec_text(&["出版社", "出 版 社", "publisher"]);
            let edition = spec_text(&["版次", "版本", "edition"]);
            let condition = if default_condition.trim().is_empty() {
                "九成新"
            } else {
                default_condition.trim()
            };
            let sell_price = price.map(|cost| suggested_price(cost, profit_rate, min_profit));
            conn.execute(
                "INSERT INTO books(capture_id, item_id, title, author, publisher, isbn, edition, condition,
                    cost_price, sell_price, status, created_at, updated_at)
                 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    capture_id,
                    if item_id.is_empty() { None } else { Some(&item_id) },
                    title,
                    author,
                    publisher,
                    isbn,
                    edition,
                    condition,
                    price,
                    sell_price,
                    "draft",
                    timestamp,
                    timestamp
                ],
            )?;
            book_id = Some(conn.last_insert_rowid());
            created = true;
        }
    } else if let Some(id) = book_id {
        let current: Option<i64> = conn
            .query_row("SELECT capture_id FROM books WHERE id = ?", [id], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        if current != Some(capture_id) {
            conn.execute(
                "UPDATE books SET capture_id = ?, updated_at = ? WHERE id = ?",
                params![capture_id, timestamp, id],
            )?;
        }
    }

    let book = match book_id {
        Some(id) => get_book(conn, id)?,
        None => None,
    };
    Ok((capture, book, created))
}

/* --------------------------------- 图书 --------------------------------- */

pub fn list_books(
    conn: &Connection,
    q: &str,
    status: &str,
    limit: i64,
) -> rusqlite::Result<Vec<Book>> {
    let mut sql = String::from(BOOK_SELECT);
    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();

    if !q.trim().is_empty() {
        conditions.push(
            "(b.title LIKE ? OR b.isbn LIKE ? OR b.author LIKE ? OR b.publisher LIKE ?)".into(),
        );
        let like = format!("%{}%", q.trim());
        for _ in 0..4 {
            values.push(Value::Text(like.clone()));
        }
    }
    if !status.trim().is_empty() {
        conditions.push("b.status = ?".into());
        values.push(Value::Text(status.trim().to_string()));
    }
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(" ORDER BY b.id DESC LIMIT ?");
    values.push(Value::Integer(limit.clamp(1, 1000)));

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), book_from_row)?;
    rows.collect()
}

pub fn get_book(conn: &Connection, id: i64) -> rusqlite::Result<Option<Book>> {
    conn.query_row(
        &format!("{} WHERE b.id = ?", BOOK_SELECT),
        [id],
        book_from_row,
    )
    .optional()
}

pub fn book_detail(conn: &Connection, id: i64) -> rusqlite::Result<Option<BookDetail>> {
    let Some(book) = get_book(conn, id)? else {
        return Ok(None);
    };
    let capture = match book.capture_id {
        Some(capture_id) => conn
            .query_row(
                "SELECT * FROM captures WHERE id = ?",
                [capture_id],
                capture_from_row,
            )
            .optional()?,
        None => None,
    };
    let images = images_for_book(conn, id)?;
    let listings = listings_for_book(conn, id)?;
    Ok(Some(BookDetail {
        book,
        capture,
        images,
        listings,
    }))
}

pub fn create_book(conn: &Connection, patch: &BookPatch) -> rusqlite::Result<Book> {
    let timestamp = now();
    let condition = patch
        .condition
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("九成新");
    conn.execute(
        "INSERT INTO books(title, author, publisher, isbn, edition, condition, cost_price,
            list_price, sell_price, stock, status, note, created_at, updated_at)
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            patch.title.clone().unwrap_or_default().trim(),
            patch.author.clone().unwrap_or_default().trim(),
            patch.publisher.clone().unwrap_or_default().trim(),
            patch.isbn.clone().unwrap_or_default().trim(),
            patch.edition.clone().unwrap_or_default().trim(),
            condition,
            patch.cost_price,
            patch.list_price,
            patch.sell_price,
            patch.stock.unwrap_or(1).max(0),
            "draft",
            patch.note.clone().unwrap_or_default().trim(),
            timestamp,
            timestamp
        ],
    )?;
    let id = conn.last_insert_rowid();
    get_book(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn update_book(
    conn: &Connection,
    id: i64,
    patch: &BookPatch,
) -> rusqlite::Result<Option<Book>> {
    let mut sets: Vec<&str> = Vec::new();
    let mut values: Vec<Value> = Vec::new();

    macro_rules! set_text {
        ($field:expr, $column:expr) => {
            if let Some(value) = &$field {
                sets.push($column);
                values.push(Value::Text(value.trim().to_string()));
            }
        };
    }

    set_text!(patch.title, "title = ?");
    set_text!(patch.author, "author = ?");
    set_text!(patch.publisher, "publisher = ?");
    set_text!(patch.isbn, "isbn = ?");
    set_text!(patch.edition, "edition = ?");
    set_text!(patch.condition, "condition = ?");
    set_text!(patch.note, "note = ?");

    for (value, column) in [
        (patch.cost_price, "cost_price = ?"),
        (patch.list_price, "list_price = ?"),
        (patch.sell_price, "sell_price = ?"),
    ] {
        if let Some(number) = value {
            sets.push(column);
            values.push(Value::Real(number));
        }
    }
    if let Some(stock) = patch.stock {
        sets.push("stock = ?");
        values.push(Value::Integer(stock.max(0)));
    }
    if let Some(status) = &patch.status {
        if ["draft", "ready", "listed", "sold"].contains(&status.as_str()) {
            sets.push("status = ?");
            values.push(Value::Text(status.clone()));
        }
    }

    if !sets.is_empty() {
        sets.push("updated_at = ?");
        values.push(Value::Text(now()));
        values.push(Value::Integer(id));
        let sql = format!("UPDATE books SET {} WHERE id = ?", sets.join(", "));
        conn.execute(&sql, params_from_iter(values.iter()))?;
    }
    get_book(conn, id)
}

pub fn book_image_rows(conn: &Connection, book_id: i64) -> rusqlite::Result<Vec<ImageRow>> {
    let mut statement = conn.prepare(
        "SELECT * FROM images WHERE book_id = ? ORDER BY is_primary DESC, sort ASC, id ASC",
    )?;
    let rows = statement.query_map([book_id], image_row_from_row)?;
    rows.collect()
}

pub fn images_for_book(conn: &Connection, book_id: i64) -> rusqlite::Result<Vec<ImageItem>> {
    Ok(book_image_rows(conn, book_id)?
        .into_iter()
        .map(image_item)
        .collect())
}

pub fn get_image_row(conn: &Connection, id: i64) -> rusqlite::Result<Option<ImageRow>> {
    conn.query_row(
        "SELECT * FROM images WHERE id = ?",
        [id],
        image_row_from_row,
    )
    .optional()
}

pub fn add_image(
    conn: &Connection,
    book_id: i64,
    source: &str,
    origin_url: &str,
    origin_file: &str,
    width: Option<i64>,
    height: Option<i64>,
) -> rusqlite::Result<ImageItem> {
    let timestamp = now();
    let sort: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort), -1) + 1 FROM images WHERE book_id = ?",
            [book_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO images(book_id, source, origin_url, origin_file, width, height, is_primary, sort, created_at)
         VALUES(?,?,?,?,?,?,0,?,?)",
        params![book_id, source, origin_url, origin_file, width, height, sort, timestamp],
    )?;
    let id = conn.last_insert_rowid();
    let has_primary: Option<i64> = conn
        .query_row(
            "SELECT id FROM images WHERE book_id = ? AND is_primary = 1",
            [book_id],
            |row| row.get(0),
        )
        .optional()?;
    if has_primary.is_none() {
        conn.execute("UPDATE images SET is_primary = 1 WHERE id = ?", [id])?;
    }
    let row = get_image_row(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    Ok(image_item(row))
}

pub fn set_primary(conn: &Connection, id: i64) -> rusqlite::Result<Option<ImageItem>> {
    let Some(row) = get_image_row(conn, id)? else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE images SET is_primary = 0 WHERE book_id = ?",
        [row.book_id],
    )?;
    conn.execute("UPDATE images SET is_primary = 1 WHERE id = ?", [id])?;
    Ok(get_image_row(conn, id)?.map(image_item))
}

pub fn set_processed(
    conn: &Connection,
    id: i64,
    file: &str,
    width: Option<i64>,
    height: Option<i64>,
) -> rusqlite::Result<Option<ImageItem>> {
    conn.execute(
        "UPDATE images SET processed_file = ?, width = COALESCE(?, width), height = COALESCE(?, height) WHERE id = ?",
        params![file, width, height, id],
    )?;
    Ok(get_image_row(conn, id)?.map(image_item))
}

pub fn delete_image_row(conn: &Connection, id: i64) -> rusqlite::Result<Option<ImageRow>> {
    let Some(row) = get_image_row(conn, id)? else {
        return Ok(None);
    };
    conn.execute("DELETE FROM images WHERE id = ?", [id])?;
    let next: Option<i64> = conn
        .query_row(
            "SELECT id FROM images WHERE book_id = ? ORDER BY sort ASC, id ASC LIMIT 1",
            [row.book_id],
            |value| value.get(0),
        )
        .optional()?;
    if let Some(next_id) = next {
        conn.execute("UPDATE images SET is_primary = 1 WHERE id = ?", [next_id])?;
    }
    Ok(Some(row))
}

pub fn reorder_images(conn: &Connection, book_id: i64, ids: &[i64]) -> rusqlite::Result<()> {
    for (index, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE images SET sort = ? WHERE id = ? AND book_id = ?",
            params![index as i64, id, book_id],
        )?;
    }
    Ok(())
}

/* --------------------------------- 发布 --------------------------------- */

pub fn listings_for_book(conn: &Connection, book_id: i64) -> rusqlite::Result<Vec<Listing>> {
    let mut statement =
        conn.prepare("SELECT * FROM listings WHERE book_id = ? ORDER BY id DESC")?;
    let rows = statement.query_map([book_id], listing_from_row)?;
    rows.collect()
}

pub fn get_listing(conn: &Connection, id: i64) -> rusqlite::Result<Option<Listing>> {
    conn.query_row(
        "SELECT * FROM listings WHERE id = ?",
        [id],
        listing_from_row,
    )
    .optional()
}

pub fn save_listing(
    conn: &Connection,
    book_id: i64,
    title: &str,
    description: &str,
    price: Option<f64>,
) -> rusqlite::Result<Listing> {
    let timestamp = now();
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM listings WHERE book_id = ? AND status IN ('draft','failed') ORDER BY id DESC LIMIT 1",
            [book_id],
            |row| row.get(0),
        )
        .optional()?;
    let id = match existing {
        Some(id) => {
            conn.execute(
                "UPDATE listings SET title=?, description=?, price=?, status='draft', error='', updated_at=? WHERE id=?",
                params![title, description, price, timestamp, id],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO listings(book_id, platform, title, description, price, status, created_at, updated_at)
                 VALUES(?, 'xianyu', ?, ?, ?, 'draft', ?, ?)",
                params![book_id, title, description, price, timestamp, timestamp],
            )?;
            conn.last_insert_rowid()
        }
    };
    get_listing(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn set_listing_status(
    conn: &Connection,
    id: i64,
    status: &str,
    url: &str,
    error: &str,
) -> rusqlite::Result<Option<Listing>> {
    let Some(listing) = get_listing(conn, id)? else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE listings SET status = ?, url = ?, error = ?, updated_at = ? WHERE id = ?",
        params![status, url, error, now(), id],
    )?;
    if status == "published" {
        conn.execute(
            "UPDATE books SET status = 'listed', updated_at = ? WHERE id = ?",
            params![now(), listing.book_id],
        )?;
    }
    get_listing(conn, id)
}

/* --------------------------------- 统计 --------------------------------- */

pub fn stats(conn: &Connection) -> rusqlite::Result<Json> {
    let (total, draft, ready, listed, sold): (i64, i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*),
            COALESCE(SUM(status = 'draft'), 0),
            COALESCE(SUM(status = 'ready'), 0),
            COALESCE(SUM(status = 'listed'), 0),
            COALESCE(SUM(status = 'sold'), 0)
         FROM books",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    let images: i64 = conn.query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))?;
    let today = format!("{}T00:00:00.000Z", Utc::now().format("%Y-%m-%d"));
    let captured_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM captures WHERE created_at >= ?",
        [today],
        |row| row.get(0),
    )?;
    let cost: f64 = conn.query_row(
        "SELECT COALESCE(SUM(cost_price), 0) FROM books",
        [],
        |row| row.get(0),
    )?;
    let listed_value: f64 = conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN status = 'listed' THEN sell_price ELSE 0 END), 0) FROM books",
        [],
        |row| row.get(0),
    )?;

    Ok(json!({
        "total": total,
        "draft": draft,
        "ready": ready,
        "listed": listed,
        "sold": sold,
        "images": images,
        "capturedToday": captured_today,
        "cost": round2(cost),
        "listedValue": round2(listed_value),
    }))
}
