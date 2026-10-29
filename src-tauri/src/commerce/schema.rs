use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub fn backup(conn: &Connection, directory: &Path) -> anyhow::Result<()> {
    let old: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='books')",
        [],
        |r| r.get(0),
    )?;
    let migrated: bool = conn.query_row(
        "SELECT COUNT(*)=4 FROM sqlite_master WHERE name IN ('collection_jobs','commerce_meta','price_searches','agent_imports')",
        [],
        |r| r.get(0),
    )?;
    if old && !migrated {
        let path = directory.join(format!(
            "shelf-before-commerce-{}.db",
            chrono::Utc::now().timestamp_millis()
        ));
        conn.execute("VACUUM INTO ?", [path.to_string_lossy().as_ref()])?;
    }
    Ok(())
}

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("SAVEPOINT commerce_migration;
        CREATE TABLE IF NOT EXISTS commerce_meta(version INTEGER PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS collection_jobs(id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, title TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'queued', stage TEXT NOT NULL DEFAULT '', request TEXT NOT NULL, result TEXT NOT NULL DEFAULT '{}',
            error TEXT NOT NULL DEFAULT '', processed INTEGER NOT NULL DEFAULT 0, total INTEGER, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_jobs_state ON collection_jobs(state,id);
        CREATE TABLE IF NOT EXISTS school_programs(id TEXT PRIMARY KEY, data TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS book_profiles(book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE, data TEXT NOT NULL DEFAULT '{}');
        CREATE TABLE IF NOT EXISTS book_ai_drafts(book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE, data TEXT NOT NULL DEFAULT '{}');
        CREATE TABLE IF NOT EXISTS book_source_analyses(book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE, data TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS price_searches(book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE, data TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS school_requirements(id TEXT PRIMARY KEY, book_id INTEGER REFERENCES books(id) ON DELETE SET NULL,
            school TEXT NOT NULL, year INTEGER NOT NULL, major TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending', data TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_requirements_book ON school_requirements(book_id);
        CREATE INDEX IF NOT EXISTS idx_requirements_school ON school_requirements(school,year,major);
        CREATE TABLE IF NOT EXISTS book_sources(id INTEGER PRIMARY KEY AUTOINCREMENT, book_id INTEGER REFERENCES books(id) ON DELETE SET NULL,
            capture_id INTEGER UNIQUE REFERENCES captures(id) ON DELETE SET NULL, is_primary INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_primary_source ON book_sources(book_id) WHERE is_primary=1 AND book_id IS NOT NULL;
        CREATE TABLE IF NOT EXISTS digital_resources(id INTEGER PRIMARY KEY AUTOINCREMENT, book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE RESTRICT,
            version TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending', data TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
            UNIQUE(book_id,version));
        CREATE TABLE IF NOT EXISTS channel_products(id INTEGER PRIMARY KEY AUTOINCREMENT, book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE RESTRICT,
            app_key TEXT NOT NULL, shop TEXT NOT NULL, product_id TEXT NOT NULL DEFAULT '', data TEXT NOT NULL, updated_at TEXT NOT NULL,
            UNIQUE(app_key,shop,book_id));
        CREATE UNIQUE INDEX IF NOT EXISTS idx_channel_remote ON channel_products(app_key,shop,product_id) WHERE product_id<>'';
        CREATE INDEX IF NOT EXISTS idx_channel_book ON channel_products(book_id);
        CREATE TABLE IF NOT EXISTS fulfillment_orders(id INTEGER PRIMARY KEY AUTOINCREMENT, app_key TEXT NOT NULL, order_no TEXT NOT NULL,
            book_id INTEGER REFERENCES books(id) ON DELETE RESTRICT, channel_id INTEGER REFERENCES channel_products(id) ON DELETE RESTRICT,
            kind TEXT NOT NULL DEFAULT 'unknown', state TEXT NOT NULL DEFAULT 'unmatched', snapshot TEXT NOT NULL,
            data TEXT NOT NULL DEFAULT '{}', updated_at TEXT NOT NULL, UNIQUE(app_key,order_no));
        CREATE TABLE IF NOT EXISTS purchase_attempts(id INTEGER PRIMARY KEY AUTOINCREMENT, fulfillment_id INTEGER NOT NULL REFERENCES fulfillment_orders(id) ON DELETE RESTRICT,
            state TEXT NOT NULL DEFAULT 'draft', data TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_fulfillment_book ON fulfillment_orders(book_id);
        CREATE INDEX IF NOT EXISTS idx_fulfillment_channel ON fulfillment_orders(channel_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_active_purchase ON purchase_attempts(fulfillment_id) WHERE state NOT IN ('canceled','replaced');
        CREATE TABLE IF NOT EXISTS commerce_events(id INTEGER PRIMARY KEY AUTOINCREMENT, entity TEXT NOT NULL, entity_id TEXT NOT NULL,
            action TEXT NOT NULL, data TEXT NOT NULL, created_at TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS delivery_records(id INTEGER PRIMARY KEY AUTOINCREMENT, fulfillment_id INTEGER NOT NULL REFERENCES fulfillment_orders(id) ON DELETE RESTRICT,
            fingerprint TEXT NOT NULL, data TEXT NOT NULL, created_at TEXT NOT NULL, UNIQUE(fulfillment_id,fingerprint));
    ")?;
    let exists: Option<i64> = conn
        .query_row(
            "SELECT version FROM commerce_meta WHERE version=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_none() {
        conn.execute("INSERT OR IGNORE INTO book_sources(book_id,capture_id,is_primary,data,updated_at)
            SELECT b.id,c.id,1,json_object('url',c.url,'title',c.title,'shop',c.shop,'price',c.price,'freight',NULL,
            'sku','','condition',b.condition,'status','pending','dropship',0,'checkedAt','','origin','migration'),b.updated_at
            FROM books b JOIN captures c ON c.id=b.capture_id ORDER BY b.id", [])?;
        conn.execute("INSERT OR IGNORE INTO channel_products(book_id,app_key,shop,product_id,data,updated_at)
            SELECT book_id,app_key,user_name,product_id,json_object('mode','physical','legacy',1,'physicalSkuId','0',
            'digitalSkuId','','title',title,'itemId',item_id,'state',status),updated_at
            FROM xianyu_tasks WHERE product_id<>'' AND book_id IS NOT NULL ORDER BY id", [])?;
        conn.execute("INSERT INTO commerce_meta(version) VALUES(?)", params![1])?;
    }
    conn.execute_batch("RELEASE commerce_migration")
}
