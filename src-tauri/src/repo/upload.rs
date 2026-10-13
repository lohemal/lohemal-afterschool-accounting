//! 업로드 파일 · 원본 행. 같은 내용(sha256)의 파일은 살아 있는 상태로 한 번만 등록된다.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{AppError, AppResult};
use crate::model::{NewUploadFile, UploadFile};
use crate::repo::audit;

const COLS: &str = "u.id, u.school_year_id, u.file_name, u.sha256, u.byte_size, u.format_code, u.version_no,
                    u.status, u.replaced_by_id, u.note, u.uploaded_at, u.applied_at, u.cancelled_at,
                    (SELECT count(*) FROM source_row r WHERE r.upload_file_id = u.id),
                    (SELECT count(*) FROM ledger_entry e WHERE e.upload_file_id = u.id)";

fn map(r: &Row) -> rusqlite::Result<UploadFile> {
    Ok(UploadFile {
        id: r.get(0)?,
        school_year_id: r.get(1)?,
        file_name: r.get(2)?,
        sha256: r.get(3)?,
        byte_size: r.get(4)?,
        format_code: r.get(5)?,
        version_no: r.get(6)?,
        status: r.get(7)?,
        replaced_by_id: r.get(8)?,
        note: r.get(9)?,
        uploaded_at: r.get(10)?,
        applied_at: r.get(11)?,
        cancelled_at: r.get(12)?,
        row_count: r.get(13)?,
        entry_count: r.get(14)?,
    })
}

pub fn list(conn: &Connection, school_year_id: i64) -> AppResult<Vec<UploadFile>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM upload_file u WHERE u.school_year_id = ?1 ORDER BY u.id DESC"
    ))?;
    let rows = st.query_map([school_year_id], map)?;
    let out: Result<Vec<_>, _> = rows.collect();
    Ok(out?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<UploadFile> {
    conn.query_row(&format!("SELECT {COLS} FROM upload_file u WHERE u.id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("업로드 파일을 찾지 못했습니다."))
}

/// 살아 있는(취소·대체되지 않은) 같은 내용의 파일이 있으면 돌려준다 — 중복 감지.
pub fn find_live_by_sha(conn: &Connection, school_year_id: i64, sha256: &str) -> AppResult<Option<UploadFile>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLS} FROM upload_file u
                  WHERE u.school_year_id = ?1 AND u.sha256 = ?2
                    AND u.status NOT IN ('cancelled', 'replaced')"
            ),
            params![school_year_id, sha256],
            map,
        )
        .optional()?)
}

pub fn register(conn: &Connection, input: &NewUploadFile) -> AppResult<UploadFile> {
    if input.sha256.len() != 64 || !input.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::invalid("파일 해시가 올바르지 않습니다."));
    }
    if let Some(dup) = find_live_by_sha(conn, input.school_year_id, &input.sha256)? {
        return Err(AppError::conflict(format!(
            "같은 내용의 파일이 이미 등록되어 있습니다: '{}' ({})",
            dup.file_name, dup.uploaded_at
        )));
    }
    // 같은 이름의 파일을 다시 올리면 판 번호를 올린다 (수정본 추적).
    let version: i64 = conn.query_row(
        "SELECT coalesce(max(version_no), 0) + 1 FROM upload_file
          WHERE school_year_id = ?1 AND file_name = ?2",
        params![input.school_year_id, input.file_name],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO upload_file (school_year_id, file_name, sha256, byte_size, format_code, version_no, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            input.school_year_id,
            input.file_name,
            input.sha256,
            input.byte_size,
            input.format_code,
            version,
            input.note.clone().unwrap_or_default()
        ],
    )?;
    let id = conn.last_insert_rowid();
    audit::write(
        conn,
        "upload.register",
        "upload_file",
        Some(id),
        &format!("파일 등록 '{}' (제{}판)", input.file_name, version),
        serde_json::json!({ "formatCode": input.format_code }),
    )?;
    get(conn, id)
}

/// 원본 행 저장. 셀 값은 해석하지 않고 JSON 배열 그대로 둔다.
pub fn add_row(
    conn: &Connection,
    upload_file_id: i64,
    sheet_name: &str,
    row_no: i64,
    cells: &serde_json::Value,
) -> AppResult<i64> {
    let text = cells.to_string();
    let hash = crate::domain::hash::sha256_hex(text.as_bytes());
    conn.execute(
        "INSERT INTO source_row (upload_file_id, sheet_name, row_no, cells_json, row_hash)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![upload_file_id, sheet_name, row_no, text, hash],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn set_status(conn: &Connection, id: i64, status: &str) -> AppResult<UploadFile> {
    let col = match status {
        "applied" => "applied_at",
        "cancelled" => "cancelled_at",
        "previewed" | "registered" => "",
        _ => return Err(AppError::invalid("알 수 없는 업로드 상태입니다.")),
    };
    let cur = get(conn, id)?;
    if cur.status == "replaced" {
        return Err(AppError::invalid("대체된 파일의 상태는 바꿀 수 없습니다."));
    }
    if col.is_empty() {
        conn.execute("UPDATE upload_file SET status = ?2 WHERE id = ?1", params![id, status])?;
    } else {
        conn.execute(
            &format!("UPDATE upload_file SET status = ?2, {col} = datetime('now', 'localtime') WHERE id = ?1"),
            params![id, status],
        )?;
    }
    audit::write(
        conn,
        &format!("upload.{status}"),
        "upload_file",
        Some(id),
        &format!("파일 '{}' 상태 {} → {}", cur.file_name, cur.status, status),
        serde_json::json!({}),
    )?;
    get(conn, id)
}

/// 잘못 올린 파일을 새 파일로 대체한다. 옛 파일은 지우지 않고 'replaced' 로 남긴다.
pub fn replace(conn: &Connection, old_id: i64, new_input: &NewUploadFile) -> AppResult<UploadFile> {
    let old = get(conn, old_id)?;
    if old.status == "replaced" || old.status == "cancelled" {
        return Err(AppError::invalid("이미 취소되었거나 대체된 파일입니다."));
    }
    conn.execute(
        "UPDATE upload_file SET status = 'replaced', cancelled_at = datetime('now', 'localtime') WHERE id = ?1",
        [old_id],
    )?;
    let new = register(conn, new_input)?;
    conn.execute(
        "UPDATE upload_file SET replaced_by_id = ?2 WHERE id = ?1",
        params![old_id, new.id],
    )?;
    audit::write(
        conn,
        "upload.replace",
        "upload_file",
        Some(old_id),
        &format!("파일 '{}' 을(를) '{}' (으)로 대체", old.file_name, new.file_name),
        serde_json::json!({ "replacedById": new.id }),
    )?;
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::repo::year;

    fn input(year_id: i64, name: &str, sha: &str) -> NewUploadFile {
        NewUploadFile {
            school_year_id: year_id,
            file_name: name.into(),
            sha256: sha.into(),
            byte_size: 1234,
            format_code: "roster_v2".into(),
            note: None,
        }
    }

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn same_content_is_rejected_until_cancelled() {
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| year::create(tx, 2026)).unwrap();
        let a = db.write(|tx| register(tx, &input(y.id, "명단.xlsx", SHA_A))).unwrap();
        assert_eq!(a.version_no, 1);

        // 이름이 달라도 내용이 같으면 막는다.
        let err = db.write(|tx| register(tx, &input(y.id, "명단(복사본).xlsx", SHA_A))).unwrap_err();
        assert_eq!(err.code, "CONFLICT");

        // 취소한 뒤에는 다시 올릴 수 있고, 취소 이력은 남는다.
        db.write(|tx| set_status(tx, a.id, "cancelled")).unwrap();
        let again = db.write(|tx| register(tx, &input(y.id, "명단.xlsx", SHA_A))).unwrap();
        assert_eq!(again.version_no, 2, "같은 이름의 두 번째 판");
        let all = db.read(|c| list(c, y.id)).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|u| u.status == "cancelled" && u.cancelled_at.is_some()));
    }

    #[test]
    fn unique_index_also_guards_direct_inserts() {
        // 응용 코드를 거치지 않는 INSERT 도 자료 모델이 막는다 (요구 8번).
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| year::create(tx, 2026)).unwrap();
        db.write(|tx| register(tx, &input(y.id, "a.xlsx", SHA_A))).unwrap();
        let err = db
            .write(|tx| {
                tx.execute(
                    "INSERT INTO upload_file (school_year_id, file_name, sha256, byte_size) VALUES (?1, 'b.xlsx', ?2, 1)",
                    params![y.id, SHA_A],
                )?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(err.code, "CONFLICT");
        assert_eq!(err.message, "같은 내용의 파일이 이미 등록되어 있습니다.");
    }

    #[test]
    fn replace_keeps_old_file_and_links_new_one() {
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| year::create(tx, 2026)).unwrap();
        let old = db.write(|tx| register(tx, &input(y.id, "명단.xlsx", SHA_A))).unwrap();
        db.write(|tx| {
            add_row(tx, old.id, "수강생명단", 2, &serde_json::json!(["코딩A", "1", "가람", "", "홍길동", "일반", 70000]))
        })
        .unwrap();
        let new = db.write(|tx| replace(tx, old.id, &input(y.id, "명단(수정).xlsx", SHA_B))).unwrap();
        let old2 = db.read(|c| get(c, old.id)).unwrap();
        assert_eq!(old2.status, "replaced");
        assert_eq!(old2.replaced_by_id, Some(new.id));
        assert_eq!(old2.row_count, 1, "원본 행은 그대로 남는다");
        // 대체된 파일은 다시 상태를 바꿀 수 없다.
        assert_eq!(db.write(|tx| set_status(tx, old.id, "applied")).unwrap_err().code, "INVALID");
        // 같은 행 번호를 두 번 넣을 수 없다.
        let err = db
            .write(|tx| add_row(tx, old.id, "수강생명단", 2, &serde_json::json!([])))
            .unwrap_err();
        assert_eq!(err.code, "CONFLICT");
    }

    #[test]
    fn bad_hash_is_rejected() {
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| year::create(tx, 2026)).unwrap();
        assert_eq!(db.write(|tx| register(tx, &input(y.id, "x.xlsx", "zz"))).unwrap_err().code, "INVALID");
    }
}
