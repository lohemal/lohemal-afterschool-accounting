//! 재원 별칭 — 엑셀의 머리글·지원유형 글자를 재원으로 잇는다.
//! 별칭은 공백을 뗀 글자로 저장·비교한다('3학년 지원금' = '3학년지원금').

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::model::FundAlias;
use crate::repo::audit;

pub fn normalize(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

pub fn list(conn: &Connection) -> AppResult<Vec<FundAlias>> {
    let mut st = conn.prepare(
        "SELECT a.id, a.fund_type_id, f.name, a.alias, a.is_builtin
           FROM fund_type_alias a JOIN fund_type f ON f.id = a.fund_type_id
          ORDER BY f.sort_order, a.alias",
    )?;
    let rows = st.query_map([], |r| {
        Ok(FundAlias {
            id: r.get(0)?,
            fund_type_id: r.get(1)?,
            fund_type_name: r.get(2)?,
            alias: r.get(3)?,
            is_builtin: r.get::<_, i64>(4)? == 1,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 글자 → 재원 id (통합된 재원은 통합 대상으로 접는다). 모르면 None — 추측하지 않는다.
pub fn resolve(conn: &Connection, text: &str) -> AppResult<Option<i64>> {
    let n = normalize(text);
    if n.is_empty() {
        return Ok(None);
    }
    let id: Option<i64> = conn
        .query_row(
            "SELECT coalesce(f.merged_into_id, f.id)
               FROM fund_type_alias a JOIN fund_type f ON f.id = a.fund_type_id
              WHERE a.alias = ?1",
            [n.clone()],
            |r| r.get(0),
        )
        .optional()?;
    if id.is_some() {
        return Ok(id);
    }
    // 재원 이름 자체도 별칭이다.
    Ok(conn
        .query_row(
            "SELECT coalesce(merged_into_id, id) FROM fund_type
              WHERE replace(replace(name, ' ', ''), char(10), '') = ?1 AND is_active = 1",
            [n],
            |r| r.get(0),
        )
        .optional()?)
}

/// 통합된 재원 id → 실제 집계 대상 id.
pub fn effective_id(conn: &Connection, fund_type_id: i64) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT coalesce(merged_into_id, id) FROM fund_type WHERE id = ?1",
        [fund_type_id],
        |r| r.get(0),
    )?)
}

pub fn save(conn: &Connection, fund_type_id: i64, alias: &str) -> AppResult<FundAlias> {
    let n = normalize(alias);
    if n.is_empty() {
        return Err(AppError::invalid("별칭을 입력해 주세요."));
    }
    let target = effective_id(conn, fund_type_id)?;
    conn.execute(
        "INSERT INTO fund_type_alias (fund_type_id, alias, is_builtin) VALUES (?1, ?2, 0)
         ON CONFLICT(alias) DO UPDATE SET fund_type_id = excluded.fund_type_id",
        params![target, n],
    )?;
    let id: i64 = conn.query_row("SELECT id FROM fund_type_alias WHERE alias = ?1", [n.clone()], |r| r.get(0))?;
    audit::write(conn, "catalog.fund_alias.save", "fund_type_alias", Some(id), &format!("재원 별칭 '{n}' 연결"), serde_json::json!({ "fundTypeId": target }))?;
    list(conn)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::new("DB", "저장한 별칭을 다시 읽지 못했습니다."))
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<()> {
    let builtin: Option<i64> = conn
        .query_row("SELECT is_builtin FROM fund_type_alias WHERE id = ?1", [id], |r| r.get(0))
        .optional()?;
    match builtin {
        None => return Err(AppError::not_found("별칭을 찾지 못했습니다.")),
        Some(1) => return Err(AppError::invalid("기본 별칭은 지울 수 없습니다. 다른 재원으로 다시 연결해 주세요.")),
        _ => {}
    }
    conn.execute("DELETE FROM fund_type_alias WHERE id = ?1", [id])?;
    audit::write(conn, "catalog.fund_alias.delete", "fund_type_alias", Some(id), "재원 별칭 삭제", serde_json::json!({}))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::repo::catalog;

    #[test]
    fn after_voucher_absorbs_g3_support_without_losing_the_row() {
        let db = Db::open_memory().unwrap();
        let g3 = db.read(|c| catalog::fund_type_by_code(c, "g3_support")).unwrap();
        let av = db.read(|c| catalog::fund_type_by_code(c, "after_voucher")).unwrap();
        assert!(!g3.is_active, "기존 행은 남되 끈다");
        assert_eq!(g3.merged_into_id, Some(av.id));
        assert_eq!(db.read(|c| effective_id(c, g3.id)).unwrap(), av.id);
        for t in ["방과후이용권", "초3 지원금", "3학년 지원금", "3학년지원금"] {
            assert_eq!(db.read(|c| resolve(c, t)).unwrap(), Some(av.id), "{t}");
        }
        let ex = db.read(|c| catalog::fund_type_by_code(c, "g3_excess")).unwrap();
        for t in ["3학년 수익자", "초3 수익자 부담금(초과금)", "초3 수익자"] {
            assert_eq!(db.read(|c| resolve(c, t)).unwrap(), Some(ex.id), "{t}");
        }
        let ben = db.read(|c| catalog::fund_type_by_code(c, "beneficiary")).unwrap();
        assert_eq!(db.read(|c| resolve(c, "수익자징수")).unwrap(), Some(ben.id));
        assert_eq!(db.read(|c| resolve(c, "일반")).unwrap(), Some(ben.id));
        assert_eq!(db.read(|c| resolve(c, "맞춤형지원\n(시간당40,000원)")).unwrap().is_some(), true);
        assert_eq!(db.read(|c| resolve(c, "방과후 이용권 + 자유수강권")).unwrap(), None, "합쳐진 유형은 추측하지 않는다");
    }

    #[test]
    fn user_alias_can_be_added_and_rename_keeps_resolution() {
        let db = Db::open_memory().unwrap();
        let av = db.read(|c| catalog::fund_type_by_code(c, "after_voucher")).unwrap();
        db.write(|tx| save(tx, av.id, "방과후 이용권 + 자유수강권")).unwrap();
        assert_eq!(db.read(|c| resolve(c, "방과후 이용권 + 자유수강권")).unwrap(), Some(av.id));
        // 이름을 바꿔도 별칭과 id 는 그대로다.
        let ren = crate::model::CatalogInput { id: Some(av.id), name: "방과후 바우처".into(), parent_id: None, is_active: None };
        db.write(|tx| catalog::fund_type_save(tx, &ren)).unwrap();
        assert_eq!(db.read(|c| resolve(c, "초3 지원금")).unwrap(), Some(av.id));
        assert_eq!(db.read(|c| resolve(c, "방과후 바우처")).unwrap(), Some(av.id));
        // g3_support 로 별칭을 걸어도 통합 대상에 붙는다.
        let g3 = db.read(|c| catalog::fund_type_by_code(c, "g3_support")).unwrap();
        let a = db.write(|tx| save(tx, g3.id, "3학년 바우처")).unwrap();
        assert_eq!(a.fund_type_id, av.id);
        let builtin = db.read(list).unwrap().into_iter().find(|a| a.is_builtin).unwrap();
        assert_eq!(db.write(|tx| delete(tx, builtin.id)).unwrap_err().code, "INVALID");
        db.write(|tx| delete(tx, a.id)).unwrap();
    }
}
