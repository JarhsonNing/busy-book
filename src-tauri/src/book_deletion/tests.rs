use super::*;
use crate::models::BookPatch;
use rusqlite::params;
use std::{fs, path::PathBuf};

struct Fixture {
    state: AppState,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("taoshu-delete-test-{}", rand::random::<u64>()));
        Self {
            state: AppState::new(path.clone(), 0).unwrap(),
            path,
        }
    }
    fn book(&self) -> i64 {
        db::create_book(
            &lock(&self.state),
            &serde_json::from_value::<BookPatch>(json!({"title":"删除验收图书"})).unwrap(),
        )
        .unwrap()
        .id
    }
    fn image(&self, id: i64) -> String {
        let file = format!("book-{id}.jpg");
        fs::write(self.state.image_dir.join(&file), b"fixture").unwrap();
        db::add_image(&lock(&self.state), id, "test", "", &file, None, None).unwrap();
        file
    }
    fn task(&self, id: i64, status: &str, product: &str) {
        lock(&self.state).execute("INSERT INTO xianyu_tasks(book_id,app_key,user_name,title,product_id,status,image_mode,created_at,updated_at) VALUES(?,'old-app','old-shop','test',?,?,'test',?,?)", params![id,product,status,db::now(),db::now()]).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let conn = std::mem::replace(
            &mut *lock(&self.state),
            Connection::open_in_memory().unwrap(),
        );
        drop(conn);
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn deletes_unpublished_drafts_and_files_but_preserves_school_evidence() {
    let f = Fixture::new();
    let id = f.book();
    let file = f.image(id);
    {
        let c = lock(&f.state);
        c.execute(r#"INSERT INTO school_requirements(id,book_id,school,year,major,state,data,updated_at) VALUES('official',?,'学校',2026,'专业','confirmed','{"sourceUrl":"https://example.org/list"}',?)"#, params![id,db::now()]).unwrap();
        c.execute(
            "INSERT INTO book_sources(book_id,is_primary,data,updated_at) VALUES(?,1,'{}',?)",
            params![id, db::now()],
        )
        .unwrap();
        c.execute("INSERT INTO digital_resources(book_id,version,data,created_at,updated_at) VALUES(?,'1','{}',?,?)", params![id,db::now(),db::now()]).unwrap();
        c.execute(r#"INSERT INTO channel_products(book_id,app_key,shop,data,updated_at) VALUES(?,'old-app','shop','{"state":"draft"}',?)"#, params![id,db::now()]).unwrap();
        // A failed web attempt can have the current browser URL without ever publishing.
        c.execute("INSERT INTO listings(book_id,status,url,created_at,updated_at) VALUES(?,'failed','https://example.org/login',?,?)", params![id,db::now(),db::now()]).unwrap();
    }
    f.task(id, "failed", "");
    let result = delete(&f.state, &[id]).unwrap();
    assert_eq!(result["deletedIds"], json!([id]));
    assert!(!f.state.image_dir.join(file).exists());
    let c = lock(&f.state);
    assert!(db::get_book(&c, id).unwrap().is_none());
    let (book, status, evidence): (Option<i64>, String, String) = c
        .query_row(
            "SELECT book_id,state,data FROM school_requirements",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(book.is_none());
    assert_eq!(status, "pending");
    assert!(evidence.contains("sourceUrl"));
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM book_sources WHERE book_id IS NULL AND is_primary=0",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM digital_resources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn protects_all_shops_orders_remote_products_and_uncertain_attempts() {
    let f = Fixture::new();
    let statements = [
        "UPDATE books SET status='listed' WHERE id=?1",
        "UPDATE books SET status='sold' WHERE id=?1",
        "INSERT INTO xianyu_orders(app_key,order_no,book_id,summary,updated_at) VALUES('other','order',?1,'{}','now')",
        "INSERT INTO fulfillment_orders(app_key,order_no,book_id,snapshot,updated_at) VALUES('other','order',?1,'{}','now')",
        "INSERT INTO channel_products(book_id,app_key,shop,product_id,data,updated_at) VALUES(?1,'other','shop','remote','{}','now')",
        "INSERT INTO channel_products(book_id,app_key,shop,data,updated_at) VALUES(?1,'other','shop','{\"itemId\":\"remote\",\"state\":\"draft\"}','now')",
        "INSERT INTO listings(book_id,status,created_at,updated_at) VALUES(?1,'published','now','now')",
        "INSERT INTO listings(book_id,status,created_at,updated_at) VALUES(?1,'filling','now','now')",
    ];
    for statement in statements {
        let id = f.book();
        let file = f.image(id);
        lock(&f.state).execute(statement, [id]).unwrap();
        assert!(!check(&lock(&f.state), id).unwrap().can_delete);
        let result = delete(&f.state, &[id]).unwrap();
        assert_eq!(result["deletedIds"], json!([]));
        assert!(!result["skipped"][0]["reason"].as_str().unwrap().is_empty());
        assert!(f.state.image_dir.join(file).exists());
    }
    for (status, remote) in [
        ("preparing", ""),
        ("unknown", ""),
        ("failed", "remote"),
        ("offline", "remote"),
        ("deleted", "remote"),
    ] {
        let id = f.book();
        f.task(id, status, remote);
        assert!(!check(&lock(&f.state), id).unwrap().can_delete);
    }
}

#[test]
fn protects_channel_linked_order_even_without_order_book_id() {
    let f = Fixture::new();
    let id = f.book();
    let c = lock(&f.state);
    c.execute("INSERT INTO channel_products(book_id,app_key,shop,data,updated_at) VALUES(?,'app','shop','{}','now')",[id]).unwrap();
    let channel = c.last_insert_rowid();
    c.execute("INSERT INTO fulfillment_orders(app_key,order_no,channel_id,snapshot,updated_at) VALUES('app','order',?,'{}','now')",[channel]).unwrap();
    assert_eq!(check(&c, id).unwrap().reason, "关联订单，不能删除");
}

#[test]
fn rechecks_at_commit_and_mixed_batch_reports_skipped_ids() {
    let f = Fixture::new();
    let draft = f.book();
    let in_flight = f.book();
    assert_eq!(
        preview(&f.state, &[in_flight]).unwrap()["items"][0]["canDelete"],
        true
    );
    f.task(in_flight, "preparing", "");
    let result = delete(&f.state, &[draft, in_flight, 999999]).unwrap();
    assert_eq!(result["deletedIds"], json!([draft]));
    assert_eq!(result["skipped"].as_array().unwrap().len(), 2);
    assert!(db::get_book(&lock(&f.state), in_flight).unwrap().is_some());
    assert_eq!(ids(vec![draft, draft]).unwrap(), vec![draft]);
    assert!(ids(vec![]).is_err());
    assert!(ids(vec![0]).is_err());
    assert!(ids(vec![1; 201]).is_err());
}

#[test]
fn database_failure_rolls_back_all_rows_and_keeps_image_files() {
    let f = Fixture::new();
    let first = f.book();
    let second = f.book();
    let a = f.image(first);
    let b = f.image(second);
    lock(&f.state).execute_batch(&format!("CREATE TRIGGER reject_book BEFORE DELETE ON books WHEN old.id={second} BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")).unwrap();
    assert!(delete(&f.state, &[first, second]).is_err());
    let c = lock(&f.state);
    for (id, file) in [(first, a), (second, b)] {
        assert!(db::get_book(&c, id).unwrap().is_some());
        assert_eq!(db::book_image_rows(&c, id).unwrap().len(), 1);
        assert!(f.state.image_dir.join(file).exists());
    }
}

#[test]
fn keeps_shared_legacy_image_file_when_another_book_uses_it() {
    let f = Fixture::new();
    let first = f.book();
    let second = f.book();
    let file = f.image(first);
    db::add_image(&lock(&f.state), second, "test", "", &file, None, None).unwrap();
    delete(&f.state, &[first]).unwrap();
    assert!(f.state.image_dir.join(file).exists());
}
