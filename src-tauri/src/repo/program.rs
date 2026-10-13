//! 프로그램 · 별칭. 업로드 파일마다 표기가 달라('코딩A' / '코딩') 대표 프로그램에 별칭을 잇는다.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::model::Program;
use crate::repo::audit;

pub fn list(conn: &Connection, school_year_id: i64) -> AppResult<Vec<Program>> {
    let mut st = conn.prepare(
        "SELECT id, school_year_id, name, note, is_active FROM program
          WHERE school_year_id = ?1 ORDER BY name",
    )?;
    let rows = st.query_map([school_year_id], |r| {
        Ok(Program {
            id: r.get(0)?,
            school_year_id: r.get(1)?,
            name: r.get(2)?,
            note: r.get(3)?,
            is_active: r.get::<_, i64>(4)? == 1,
            aliases: Vec::new(),
        })
    })?;
    let mut out: Vec<Program> = rows.collect::<Result<_, _>>()?;
    let mut st = conn.prepare(
        "SELECT alias FROM program_alias WHERE program_id = ?1 ORDER BY alias",
    )?;
    for p in out.iter_mut() {
        let rows = st.query_map([p.id], |r| r.get::<_, String>(0))?;
        p.aliases = rows.collect::<Result<_, _>>()?;
    }
    Ok(out)
}

pub fn create(conn: &Connection, school_year_id: i64, name: &str) -> AppResult<i64> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::invalid("프로그램 이름을 입력해 주세요."));
    }
    conn.execute(
        "INSERT INTO program (school_year_id, name) VALUES (?1, ?2)",
        params![school_year_id, n],
    )?;
    let id = conn.last_insert_rowid();
    audit::write(
        conn,
        "program.create",
        "program",
        Some(id),
        &format!("프로그램 '{n}' 등록"),
        serde_json::json!({}),
    )?;
    Ok(id)
}

pub fn add_alias(conn: &Connection, school_year_id: i64, program_id: i64, alias: &str) -> AppResult<i64> {
    let a = alias.trim();
    if a.is_empty() {
        return Err(AppError::invalid("별칭을 입력해 주세요."));
    }
    conn.execute(
        "INSERT INTO program_alias (school_year_id, program_id, alias) VALUES (?1, ?2, ?3)",
        params![school_year_id, program_id, a],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 별칭 또는 대표 이름으로 프로그램을 찾는다. 없으면 None (추측하지 않는다).
pub fn resolve(conn: &Connection, school_year_id: i64, text: &str) -> AppResult<Option<i64>> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let by_name: Option<i64> = conn
        .query_row(
            "SELECT id FROM program WHERE school_year_id = ?1 AND name = ?2",
            params![school_year_id, t],
            |r| r.get(0),
        )
        .optional()?;
    if by_name.is_some() {
        return Ok(by_name);
    }
    Ok(conn
        .query_row(
            "SELECT program_id FROM program_alias WHERE school_year_id = ?1 AND alias = ?2",
            params![school_year_id, t],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::repo::year;

    #[test]
    fn alias_resolves_to_program_and_is_unique_per_year() {
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| year::create(tx, 2026)).unwrap();
        let coding = db.write(|tx| create(tx, y.id, "코딩")).unwrap();
        db.write(|tx| add_alias(tx, y.id, coding, "코딩A")).unwrap();
        db.write(|tx| add_alias(tx, y.id, coding, "코딩B")).unwrap();
        assert_eq!(db.read(|c| resolve(c, y.id, "코딩")).unwrap(), Some(coding));
        assert_eq!(db.read(|c| resolve(c, y.id, " 코딩A ")).unwrap(), Some(coding));
        assert_eq!(db.read(|c| resolve(c, y.id, "컴퓨터")).unwrap(), None);

        let robot = db.write(|tx| create(tx, y.id, "로봇과학")).unwrap();
        let err = db.write(|tx| add_alias(tx, y.id, robot, "코딩A")).unwrap_err();
        assert_eq!(err.code, "CONFLICT");

        let list = db.read(|c| list(c, y.id)).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].aliases, vec!["코딩A", "코딩B"]);
    }
}
