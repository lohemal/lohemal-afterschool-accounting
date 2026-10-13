//! SQLite 연결 관리.
//!
//! 1인 사용 데스크톱 앱이므로 커넥션 풀 대신 `Mutex<Connection>` 하나로 충분하다.
//! 모든 DB 접근은 `Db::read` / `Db::write` 를 통해서만 이루어진다.
//!
//! **주의: `Db::read` 를 `Db::write` 안에서 부르면 교착된다.** `Mutex` 는 재진입하지
//! 않는다. 쓰기 클로저 안에서는 받은 `Transaction` 만 쓴다.

pub mod migrate;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub struct Db {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Db {
    /// DB 파일을 열고 필요한 마이그레이션을 적용한다.
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut conn = Connection::open(path).map_err(|e| {
            AppError::new(
                "DB_OPEN",
                "자료 파일을 열지 못했습니다. 프로그램을 다시 시작해 주세요.",
            )
            .detail(format!("{} :: {e}", path.display()))
        })?;
        setup_conn(&conn)?;
        migrate::run(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        })
    }

    /// 테스트용 메모리 DB.
    #[cfg(test)]
    pub fn open_memory() -> AppResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        setup_conn(&conn)?;
        migrate::run(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn data_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// 읽기 전용 작업.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| AppError::new("DB_LOCK", "자료 접근이 잠겨 있습니다."))?;
        f(&guard)
    }

    /// 파일 교체 작업(복원용). 잠금을 쥔 채 연결을 닫고(WAL 체크포인트) `f` 가 파일을 바꾼 뒤 다시 연다.
    /// 잠금을 쥐고 있는 동안 다른 읽기·쓰기는 모두 기다린다. `f` 가 실패해도 원래 파일을 다시 연다.
    /// 다시 열기에 실패하면 오류를 돌려주고 연결은 비어 있는 메모리 DB 로 남는다 — 호출자가 원상 복구 뒤 `reopen` 을 부른다.
    pub fn with_file_swap(&self, f: impl FnOnce(&Path) -> AppResult<()>) -> AppResult<()> {
        if self.path.as_os_str() == ":memory:" {
            return Err(AppError::new("DB_SWAP", "메모리 DB 는 파일을 교체할 수 없습니다."));
        }
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| AppError::new("DB_LOCK", "자료 접근이 잠겨 있습니다."))?;
        let old = std::mem::replace(&mut *guard, Connection::open_in_memory()?);
        old.close().map_err(|(_, e)| AppError::new("DB_CLOSE", "자료 파일을 닫지 못했습니다.").detail(e.to_string()))?;
        let result = f(&self.path);
        let reopened = open_file_conn(&self.path);
        match (result, reopened) {
            (Ok(()), Ok(c)) => {
                *guard = c;
                Ok(())
            }
            (Err(e), Ok(c)) => {
                *guard = c;
                Err(e)
            }
            (Ok(()), Err(e)) => Err(e),
            (Err(e), Err(e2)) => Err(e.detail(format!("다시 열기도 실패: {}", e2.message))),
        }
    }

    /// 비어 있는 연결 상태에서 파일을 다시 연다 (복원 원상 복구 뒤).
    pub fn reopen(&self) -> AppResult<()> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| AppError::new("DB_LOCK", "자료 접근이 잠겨 있습니다."))?;
        *guard = open_file_conn(&self.path)?;
        Ok(())
    }

    /// 쓰기 작업. 트랜잭션으로 감싸고, 오류가 나면 통째로 되돌린다.
    pub fn write<T>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| AppError::new("DB_LOCK", "자료 접근이 잠겨 있습니다."))?;
        let tx = guard.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }
}

fn open_file_conn(path: &Path) -> AppResult<Connection> {
    let mut conn = Connection::open(path).map_err(|e| AppError::new("DB_OPEN", "자료 파일을 열지 못했습니다.").detail(format!("{} :: {e}", path.display())))?;
    setup_conn(&conn)?;
    migrate::run(&mut conn)?;
    Ok(conn)
}

fn setup_conn(conn: &Connection) -> AppResult<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_db_opens_with_latest_schema() {
        let db = Db::open_memory().unwrap();
        let v = db.read(|c| Ok(migrate::current_version(c)?)).unwrap();
        assert_eq!(v, migrate::latest_version());
    }

    #[test]
    fn file_db_persists_across_reopen() {
        // 7. 재실행 후 유지 — 파일 DB 를 닫았다 다시 열어도 자료가 남는다.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("asaccounting.db");
        {
            let db = Db::open(&path).unwrap();
            db.write(|tx| {
                tx.execute(
                    "INSERT INTO app_setting (key, value) VALUES ('school_name', '시험초등학교')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let name: String = db
            .read(|c| {
                Ok(c.query_row(
                    "SELECT value FROM app_setting WHERE key = 'school_name'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(name, "시험초등학교");
        let v = db.read(|c| Ok(migrate::current_version(c)?)).unwrap();
        assert_eq!(v, migrate::latest_version());
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let db = Db::open_memory().unwrap();
        let err = db
            .write(|tx| {
                tx.execute(
                    "INSERT INTO fiscal_month (school_year_id, ym, seq) VALUES (999, '2026-03', 1)",
                    [],
                )?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(err.code, "INVALID", "{err:?}");
    }
}
