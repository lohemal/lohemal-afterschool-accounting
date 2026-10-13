//! 재원 유형 · 금액 항목. 기본 항목은 이름만 바꿀 수 있고, 사용자 항목은 더하거나 끌 수 있다.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{AppError, AppResult};
use crate::model::{AmountItem, Catalog, CatalogInput, FundType};
use crate::repo::audit;

const FUND_COLS: &str = "id, code, name, parent_id, merged_into_id, sort_order, is_builtin, is_active";

fn map_fund(r: &Row) -> rusqlite::Result<FundType> {
    Ok(FundType {
        id: r.get(0)?,
        code: r.get(1)?,
        name: r.get(2)?,
        parent_id: r.get(3)?,
        merged_into_id: r.get(4)?,
        sort_order: r.get(5)?,
        is_builtin: r.get::<_, i64>(6)? == 1,
        is_active: r.get::<_, i64>(7)? == 1,
    })
}

fn map_item(r: &Row) -> rusqlite::Result<AmountItem> {
    Ok(AmountItem {
        id: r.get(0)?,
        code: r.get(1)?,
        name: r.get(2)?,
        sort_order: r.get(3)?,
        is_builtin: r.get::<_, i64>(4)? == 1,
        is_active: r.get::<_, i64>(5)? == 1,
    })
}

pub fn fund_types(conn: &Connection) -> AppResult<Vec<FundType>> {
    let mut st = conn.prepare(&format!("SELECT {FUND_COLS} FROM fund_type ORDER BY sort_order, id"))?;
    let rows = st.query_map([], map_fund)?;
    let out: Result<Vec<_>, _> = rows.collect();
    Ok(out?)
}

pub fn amount_items(conn: &Connection) -> AppResult<Vec<AmountItem>> {
    let mut st = conn.prepare(
        "SELECT id, code, name, sort_order, is_builtin, is_active
           FROM amount_item ORDER BY sort_order, id",
    )?;
    let rows = st.query_map([], map_item)?;
    let out: Result<Vec<_>, _> = rows.collect();
    Ok(out?)
}

pub fn fund_type_by_code(conn: &Connection, code: &str) -> AppResult<FundType> {
    conn.query_row(&format!("SELECT {FUND_COLS} FROM fund_type WHERE code = ?1"), [code], map_fund)
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("재원 '{code}' 이(가) 없습니다.")))
}

pub fn amount_item_by_code(conn: &Connection, code: &str) -> AppResult<AmountItem> {
    conn.query_row(
        "SELECT id, code, name, sort_order, is_builtin, is_active FROM amount_item WHERE code = ?1",
        [code],
        map_item,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found(format!("금액 항목 '{code}' 이(가) 없습니다.")))
}

pub fn all(conn: &Connection) -> AppResult<Catalog> {
    Ok(Catalog {
        fund_types: fund_types(conn)?,
        amount_items: amount_items(conn)?,
    })
}

fn clean_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::invalid("이름을 입력해 주세요."));
    }
    if n.chars().count() > 40 {
        return Err(AppError::invalid("이름은 40자 이내로 적어 주세요."));
    }
    Ok(n.to_string())
}

fn next_custom_code(conn: &Connection, table: &str) -> AppResult<String> {
    let n: i64 = conn.query_row(
        &format!("SELECT count(*) FROM {table} WHERE is_builtin = 0"),
        [],
        |r| r.get(0),
    )?;
    Ok(format!("custom_{}", n + 1))
}

/// 재원 유형 저장. id 가 있으면 이름·상위·사용 여부 수정, 없으면 사용자 재원 추가.
pub fn fund_type_save(conn: &Connection, input: &CatalogInput) -> AppResult<FundType> {
    let name = clean_name(&input.name)?;
    let id = match input.id {
        Some(id) => {
            let cur = conn
                .query_row(&format!("SELECT {FUND_COLS} FROM fund_type WHERE id = ?1"), [id], map_fund)
                .optional()?
                .ok_or_else(|| AppError::not_found("재원을 찾지 못했습니다."))?;
            if input.parent_id == Some(id) {
                return Err(AppError::invalid("자기 자신을 상위 재원으로 둘 수 없습니다."));
            }
            let active = input.is_active.unwrap_or(cur.is_active);
            if cur.is_builtin && !active && cur.merged_into_id.is_none() {
                return Err(AppError::invalid("기본 재원은 끌 수 없습니다. 이름만 바꿀 수 있습니다."));
            }
            conn.execute(
                "UPDATE fund_type SET name = ?2, parent_id = ?3, is_active = ?4 WHERE id = ?1",
                params![id, name, input.parent_id, active as i64],
            )?;
            audit::write(conn, "catalog.fund_type.update", "fund_type", Some(id), &format!("재원 '{}' → '{}'", cur.name, name), serde_json::json!({}))?;
            id
        }
        None => {
            let code = next_custom_code(conn, "fund_type")?;
            let order: i64 = conn.query_row(
                "SELECT coalesce(max(sort_order), 0) + 10 FROM fund_type WHERE code <> 'other'",
                [],
                |r| r.get(0),
            )?;
            conn.execute(
                "INSERT INTO fund_type (code, name, parent_id, sort_order, is_builtin, is_active)
                 VALUES (?1, ?2, ?3, ?4, 0, 1)",
                params![code, name, input.parent_id, order],
            )?;
            let id = conn.last_insert_rowid();
            audit::write(conn, "catalog.fund_type.create", "fund_type", Some(id), &format!("재원 '{name}' 추가"), serde_json::json!({}))?;
            id
        }
    };
    conn.query_row(&format!("SELECT {FUND_COLS} FROM fund_type WHERE id = ?1"), [id], map_fund)
        .map_err(Into::into)
}

pub fn amount_item_save(conn: &Connection, input: &CatalogInput) -> AppResult<AmountItem> {
    let name = clean_name(&input.name)?;
    let id = match input.id {
        Some(id) => {
            let cur = conn
                .query_row(
                    "SELECT id, code, name, sort_order, is_builtin, is_active FROM amount_item WHERE id = ?1",
                    [id],
                    map_item,
                )
                .optional()?
                .ok_or_else(|| AppError::not_found("금액 항목을 찾지 못했습니다."))?;
            let active = input.is_active.unwrap_or(cur.is_active);
            if cur.is_builtin && !active {
                return Err(AppError::invalid("기본 항목은 끌 수 없습니다. 이름만 바꿀 수 있습니다."));
            }
            conn.execute(
                "UPDATE amount_item SET name = ?2, is_active = ?3 WHERE id = ?1",
                params![id, name, active as i64],
            )?;
            audit::write(conn, "catalog.amount_item.update", "amount_item", Some(id), &format!("금액 항목 '{}' → '{}'", cur.name, name), serde_json::json!({}))?;
            id
        }
        None => {
            let code = next_custom_code(conn, "amount_item")?;
            let order: i64 = conn.query_row(
                "SELECT coalesce(max(sort_order), 0) + 10 FROM amount_item WHERE code <> 'other'",
                [],
                |r| r.get(0),
            )?;
            conn.execute(
                "INSERT INTO amount_item (code, name, sort_order, is_builtin, is_active)
                 VALUES (?1, ?2, ?3, 0, 1)",
                params![code, name, order],
            )?;
            let id = conn.last_insert_rowid();
            audit::write(conn, "catalog.amount_item.create", "amount_item", Some(id), &format!("금액 항목 '{name}' 추가"), serde_json::json!({}))?;
            id
        }
    };
    conn.query_row(
        "SELECT id, code, name, sort_order, is_builtin, is_active FROM amount_item WHERE id = ?1",
        [id],
        map_item,
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn builtin_catalog_is_seeded_in_order() {
        let db = Db::open_memory().unwrap();
        let cat = db.read(all).unwrap();
        let codes: Vec<&str> = cat.fund_types.iter().map(|f| f.code.as_str()).collect();
        assert_eq!(
            codes,
            ["beneficiary", "free_voucher", "g3_support", "g3_excess", "after_voucher", "custom_support", "other"]
        );
        let items: Vec<&str> = cat.amount_items.iter().map(|f| f.code.as_str()).collect();
        assert_eq!(items, ["instructor", "admin", "textbook", "material", "textbook_material", "other"]);
    }

    #[test]
    fn custom_fund_type_can_be_added_renamed_and_disabled() {
        let db = Db::open_memory().unwrap();
        let input = CatalogInput { id: None, name: " 교육청 특별지원 ".into(), parent_id: None, is_active: None };
        let f = db.write(|tx| fund_type_save(tx, &input)).unwrap();
        assert_eq!(f.name, "교육청 특별지원");
        assert_eq!(f.code, "custom_1");
        assert!(!f.is_builtin);
        let cat = db.read(all).unwrap();
        let pos_custom = cat.fund_types.iter().position(|x| x.id == f.id).unwrap();
        let pos_other = cat.fund_types.iter().position(|x| x.code == "other").unwrap();
        assert!(pos_custom < pos_other);

        let off = CatalogInput { id: Some(f.id), name: "특별지원".into(), parent_id: None, is_active: Some(false) };
        let f2 = db.write(|tx| fund_type_save(tx, &off)).unwrap();
        assert_eq!(f2.name, "특별지원");
        assert!(!f2.is_active);
    }

    #[test]
    fn builtin_cannot_be_disabled_but_can_be_renamed() {
        let db = Db::open_memory().unwrap();
        let g3 = db.read(|c| fund_type_by_code(c, "g3_excess")).unwrap();
        let off = CatalogInput { id: Some(g3.id), name: g3.name.clone(), parent_id: g3.parent_id, is_active: Some(false) };
        assert_eq!(db.write(|tx| fund_type_save(tx, &off)).unwrap_err().code, "INVALID");
        let ren = CatalogInput { id: Some(g3.id), name: "초3 초과금".into(), parent_id: g3.parent_id, is_active: None };
        let saved = db.write(|tx| fund_type_save(tx, &ren)).unwrap();
        assert_eq!(saved.name, "초3 초과금");
        assert_eq!(saved.parent_id, g3.parent_id);

        let item = db.read(|c| amount_item_by_code(c, "textbook_material")).unwrap();
        let bad = CatalogInput { id: Some(item.id), name: "".into(), parent_id: None, is_active: None };
        assert_eq!(db.write(|tx| amount_item_save(tx, &bad)).unwrap_err().code, "INVALID");
        let add = CatalogInput { id: None, name: "간식비".into(), parent_id: None, is_active: None };
        let added = db.write(|tx| amount_item_save(tx, &add)).unwrap();
        assert_eq!(added.code, "custom_1");
    }
}
