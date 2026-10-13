//! 학생 식별. 이름만으로 합치지 않고 학년·반·번호·이름 네 가지가 모두 같아야 같은 학생이다.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppResult;
use crate::excel::parse::StudentKey;

/// 있으면 그 id, 없으면 만든다. 이름이 없으면 None (식별 불가).
pub fn upsert(conn: &Connection, school_year_id: i64, key: &StudentKey) -> AppResult<Option<i64>> {
    let Some(identity) = key.identity_key() else { return Ok(None) };
    let found: Option<i64> = conn
        .query_row(
            "SELECT id FROM student WHERE school_year_id = ?1 AND identity_key = ?2",
            params![school_year_id, identity],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        return Ok(Some(id));
    }
    // 학년을 모르면 0 으로 둘 수 없다(CHECK 1~6). 학년 없는 학생은 학생 표에 넣지 않는다.
    let Some(grade) = key.grade.filter(|g| (1..=6).contains(g)) else { return Ok(None) };
    conn.execute(
        "INSERT INTO student (school_year_id, grade, class_name, student_no, name, identity_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![school_year_id, grade, key.class_name, key.student_no, key.name, identity],
    )?;
    Ok(Some(conn.last_insert_rowid()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::repo::year;

    #[test]
    fn same_name_different_class_is_a_different_student() {
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| year::create(tx, 2026)).unwrap();
        let a = StudentKey { grade: Some(1), class_name: "가람".into(), student_no: "3".into(), name: "가명A".into() };
        let b = StudentKey { grade: Some(2), class_name: "가람".into(), student_no: "3".into(), name: "가명A".into() };
        let ia = db.write(|tx| upsert(tx, y.id, &a)).unwrap().unwrap();
        let ib = db.write(|tx| upsert(tx, y.id, &b)).unwrap().unwrap();
        assert_ne!(ia, ib);
        assert_eq!(db.write(|tx| upsert(tx, y.id, &a)).unwrap(), Some(ia), "같은 키는 같은 학생");
        let no_name = StudentKey { grade: Some(1), ..Default::default() };
        assert_eq!(db.write(|tx| upsert(tx, y.id, &no_name)).unwrap(), None);
        let no_grade = StudentKey { name: "가명Z".into(), ..Default::default() };
        assert_eq!(db.write(|tx| upsert(tx, y.id, &no_grade)).unwrap(), None);
    }
}
