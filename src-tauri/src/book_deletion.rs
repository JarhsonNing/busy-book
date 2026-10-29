use crate::{
    api::ApiError,
    db, images,
    state::{lock, AppState},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: i64,
    pub title: String,
    pub can_delete: bool,
    pub reason: String,
}

// Check every shop, including old connections. A failed attempt without a remote
// identifier is a local draft; an uncertain result must be reconciled first.
pub fn check(conn: &Connection, id: i64) -> rusqlite::Result<Check> {
    let row = conn.query_row(
        "SELECT title, CASE
        WHEN EXISTS(SELECT 1 FROM xianyu_orders WHERE book_id=b.id)
          OR EXISTS(SELECT 1 FROM fulfillment_orders WHERE book_id=b.id
            OR channel_id IN (SELECT id FROM channel_products WHERE book_id=b.id))
          THEN '关联订单，不能删除'
        WHEN status NOT IN ('draft','ready') THEN '已发布或已售出，不能删除'
        WHEN EXISTS(SELECT 1 FROM xianyu_tasks WHERE book_id=b.id AND (product_id<>'' OR item_id<>''))
          OR EXISTS(SELECT 1 FROM channel_products WHERE book_id=b.id AND
            (product_id<>'' OR coalesce(json_extract(data,'$.productId'),'')<>'' OR coalesce(json_extract(data,'$.itemId'),'')<>''))
          THEN '已关联平台商品，不能删除'
        WHEN EXISTS(SELECT 1 FROM xianyu_tasks WHERE book_id=b.id AND status<>'failed')
          OR EXISTS(SELECT 1 FROM channel_products WHERE book_id=b.id AND coalesce(json_extract(data,'$.state'),'draft') NOT IN ('draft','ready','failed'))
          THEN '发布中或结果待核查，不能删除'
        WHEN EXISTS(SELECT 1 FROM listings WHERE book_id=b.id AND status NOT IN ('draft','failed'))
          THEN '网页发布中或已发布，不能删除'
        ELSE '' END FROM books b WHERE id=?",
        [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    ).optional()?;
    Ok(match row {
        Some((title, reason)) => Check {
            id,
            title,
            can_delete: reason.is_empty(),
            reason,
        },
        None => Check {
            id,
            title: format!("图书 #{id}"),
            can_delete: false,
            reason: "图书不存在或已删除".into(),
        },
    })
}

pub fn ids(ids: Vec<i64>) -> Result<Vec<i64>, ApiError> {
    if ids.is_empty() || ids.len() > 200 || ids.iter().any(|id| *id <= 0) {
        return Err(ApiError::bad("每次选择 1–200 本有效图书"));
    }
    let mut unique = Vec::new();
    for id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    Ok(unique)
}

pub fn preview(state: &AppState, ids: &[i64]) -> Result<Value, ApiError> {
    let conn = lock(state);
    let items = ids
        .iter()
        .map(|id| check(&conn, *id))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"items":items}))
}

pub fn delete(state: &AppState, ids: &[i64]) -> Result<Value, ApiError> {
    let mut conn = lock(state);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut files = Vec::new();
    let mut deleted = Vec::new();
    let mut skipped = Vec::new();
    for &id in ids {
        let result = check(&tx, id)?;
        if !result.can_delete {
            skipped.push(result);
            continue;
        }
        for image in db::book_image_rows(&tx, id)? {
            files.push(image.origin_file);
            files.push(image.processed_file);
        }
        // Keep official requirements and capture evidence available for rematching.
        tx.execute(
            "UPDATE school_requirements SET state='pending',updated_at=? WHERE book_id=?",
            rusqlite::params![db::now(), id],
        )?;
        tx.execute("UPDATE book_sources SET is_primary=0 WHERE book_id=?", [id])?;
        tx.execute("DELETE FROM channel_products WHERE book_id=?", [id])?;
        tx.execute("DELETE FROM digital_resources WHERE book_id=?", [id])?;
        tx.execute("DELETE FROM books WHERE id=?", [id])?;
        deleted.push(id);
    }
    // Retain files still referenced by another image row, including legacy data.
    files.sort();
    files.dedup();
    let mut removable = Vec::new();
    for file in files.into_iter().filter(|f| !f.is_empty()) {
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM images WHERE origin_file=?1 OR processed_file=?1)",
            [&file],
            |r| r.get(0),
        )?;
        if !used {
            removable.push(file);
        }
    }
    tx.commit()?;
    // Never touch files before the whole database mutation commits. Keep the
    // connection lock until cleanup finishes so another writer cannot reuse them.
    for file in removable {
        images::remove_file(&state.image_dir, &file);
    }
    drop(conn);
    let mut context = state.import_ctx.lock().unwrap_or_else(|e| e.into_inner());
    if context
        .as_ref()
        .is_some_and(|c| deleted.contains(&c.book_id))
    {
        *context = None;
    }
    Ok(json!({"deletedIds":deleted,"skipped":skipped}))
}

#[cfg(test)]
mod tests;
