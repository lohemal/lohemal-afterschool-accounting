//! 앱 전역 오류 타입.
//!
//! 화면에 그대로 보여줄 한국어 문장(`message`)과 개발자용 원문(`detail`)을 나눈다.
//! Rust 쪽에서는 `panic!`을 쓰지 않는다 — 한 화면의 오류가 앱 전체를 끄면 안 된다.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            detail: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// 사용자 입력이 잘못된 경우 — 화면에 그대로 보여준다.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("NOT_FOUND", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("CONFLICT", message)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        let text = e.to_string();
        // SQLite 는 위반한 컬럼 이름을 알려 준다 (`UNIQUE constraint failed: school_year.year`).
        if text.contains("UNIQUE constraint failed") {
            let msg = if text.contains("school_year.year") {
                "같은 학년도가 이미 있습니다."
            } else if text.contains("upload_file.sha256") || text.contains("ux_upload_file_live_sha") {
                "같은 내용의 파일이 이미 등록되어 있습니다."
            } else if text.contains("program.name") {
                "같은 이름의 프로그램이 이미 있습니다."
            } else if text.contains("program_alias.alias") {
                "같은 별칭이 이미 다른 프로그램에 연결되어 있습니다."
            } else if text.contains("student.identity_key") {
                "같은 학년·반·번호·이름의 학생이 이미 있습니다."
            } else if text.contains("fund_type.code") || text.contains("amount_item.code") {
                "같은 코드가 이미 있습니다."
            } else if text.contains("source_row") {
                "같은 원본 행이 이미 저장되어 있습니다."
            } else {
                "이미 등록된 자료입니다."
            };
            return AppError::new("CONFLICT", msg).detail(text);
        }
        if text.contains("LEDGER_IMMUTABLE") {
            return AppError::new(
                "LEDGER_IMMUTABLE",
                "등록된 거래의 금액·분류는 고칠 수 없습니다. 취소하거나 대체 거래를 등록해 주세요.",
            )
            .detail(text);
        }
        if text.contains("LEDGER_NO_DELETE") {
            return AppError::new(
                "LEDGER_NO_DELETE",
                "등록된 거래는 지울 수 없습니다. 취소 처리를 사용해 주세요.",
            )
            .detail(text);
        }
        if text.contains("CHECK constraint failed") {
            return AppError::invalid("입력값이 허용 범위를 벗어났습니다.").detail(text);
        }
        if text.contains("FOREIGN KEY constraint failed") {
            return AppError::invalid("연결된 자료가 없거나 이미 지워졌습니다.").detail(text);
        }
        AppError::new("DB", "자료를 처리하지 못했습니다.").detail(text)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::new("IO", "파일을 읽거나 쓰지 못했습니다.").detail(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::new("JSON", "자료 형식을 변환하지 못했습니다.").detail(e.to_string())
    }
}
