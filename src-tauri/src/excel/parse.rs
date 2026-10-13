//! 엑셀 → 정규화 행. 양식마다 다른 열 배치를 **한 가지 모양**(`ParsedRow`)으로 바꾼다.
//!
//! 여기서는 프로그램·재원·학생을 **해석하지 않는다**(글자만 뽑는다). DB 와 맞추는 일은
//! `repo::upload::preview` 가 한다. 금액은 원문 그대로이며 프로그램이 바꾸지 않는다.

use serde::{Deserialize, Serialize};

use super::detect::FormatCode;
use super::{amount_of, Book, Sheet};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// info · check · error
    pub level: String,
    pub message: String,
}

impl Issue {
    pub fn info(m: impl Into<String>) -> Self {
        Issue { level: "info".into(), message: m.into() }
    }
    pub fn check(m: impl Into<String>) -> Self {
        Issue { level: "check".into(), message: m.into() }
    }
    pub fn error(m: impl Into<String>) -> Self {
        Issue { level: "error".into(), message: m.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StudentKey {
    pub grade: Option<i32>,
    pub class_name: String,
    pub student_no: String,
    pub name: String,
}

impl StudentKey {
    pub fn is_blank(&self) -> bool {
        self.grade.is_none() && self.class_name.is_empty() && self.student_no.is_empty() && self.name.is_empty()
    }
    /// 이름이 있어야 학생으로 본다. 동명이인은 학년·반·번호까지 같아야 같은 학생이다.
    pub fn identity_key(&self) -> Option<String> {
        if self.name.is_empty() {
            return None;
        }
        Some(format!(
            "{}|{}|{}|{}",
            self.grade.map(|g| g.to_string()).unwrap_or_default(),
            self.class_name,
            self.student_no,
            self.name
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedRow {
    pub seq: i64,
    pub sheet: String,
    pub row_no: u32,
    pub program_text: String,
    /// 병합 셀에서 이어받은 프로그램명인가
    pub program_inherited: bool,
    /// 공란 프로그램명에 대해 바로 위 행이 제안하는 이름 (사용자 확인용)
    pub program_suggestion: String,
    pub student: Option<StudentKey>,
    pub fund_text: String,
    /// 시트 구조로 재원이 확정된 경우의 코드 (징수명단의 재원별 시트)
    pub fund_code: String,
    pub item_code: String,
    pub entry_kind: String,
    pub flow: String,
    pub amount: i64,
    pub counterparty: String,
    pub issues: Vec<Issue>,
}

impl ParsedRow {
    pub fn worst_level(&self) -> &'static str {
        if self.issues.iter().any(|i| i.level == "error") {
            "error"
        } else if self.issues.iter().any(|i| i.level == "check") {
            "check"
        } else {
            "ok"
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRow {
    pub sheet: String,
    pub row_no: u32,
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parsed {
    pub format: FormatCode,
    pub rows: Vec<ParsedRow>,
    pub source_rows: Vec<SourceRow>,
    /// 파일 전체에 대한 검사 결과 (합계 행 대조 등)
    pub file_issues: Vec<Issue>,
    /// 읽은 원본 행 수 (머리글·합계 제외)
    pub data_row_count: u32,
    /// 원장에서 뺀 행(취소 수강 등) 수
    pub excluded_row_count: u32,
}

pub fn parse(book: &Book, format: FormatCode) -> AppResult<Parsed> {
    match format {
        FormatCode::RosterManager => super::formats::roster_manager(book),
        FormatCode::InstructorPay => super::formats::instructor_pay(book),
        FormatCode::VendorPay => super::formats::vendor_pay(book),
        FormatCode::CollectInitial => super::formats::collect_initial(book),
        FormatCode::RefundExtraS1 => super::formats::refund_extra_s1(book),
        FormatCode::SettleS1 => super::formats::settle_s1(book),
        FormatCode::RosterS1 => super::formats::roster_s1(book),
    }
}

// ── 파서 공용 도우미 ───────────────────────────────────────────────────────

pub(super) struct Ctx {
    pub rows: Vec<ParsedRow>,
    pub source_rows: Vec<SourceRow>,
    pub file_issues: Vec<Issue>,
    pub data_row_count: u32,
    pub excluded_row_count: u32,
}

impl Ctx {
    pub fn new() -> Self {
        Ctx { rows: Vec::new(), source_rows: Vec::new(), file_issues: Vec::new(), data_row_count: 0, excluded_row_count: 0 }
    }

    pub fn keep_source(&mut self, sheet: &Sheet, row: u32) {
        let cells = (1..=sheet.max_col).map(|c| sheet.text(row, c)).collect();
        self.source_rows.push(SourceRow { sheet: sheet.name.clone(), row_no: row, cells });
    }

    pub fn push(&mut self, mut r: ParsedRow) {
        r.seq = self.rows.len() as i64 + 1;
        self.rows.push(r);
    }

    pub fn finish(self, format: FormatCode) -> Parsed {
        Parsed {
            format,
            rows: self.rows,
            source_rows: self.source_rows,
            file_issues: self.file_issues,
            data_row_count: self.data_row_count,
            excluded_row_count: self.excluded_row_count,
        }
    }
}

pub(super) fn blank_row(sheet: &str, row_no: u32) -> ParsedRow {
    ParsedRow {
        seq: 0,
        sheet: sheet.to_string(),
        row_no,
        program_text: String::new(),
        program_inherited: false,
        program_suggestion: String::new(),
        student: None,
        fund_text: String::new(),
        fund_code: String::new(),
        item_code: String::new(),
        entry_kind: String::new(),
        flow: String::new(),
        amount: 0,
        counterparty: String::new(),
        issues: Vec::new(),
    }
}

pub(super) fn sheet_or_err<'a>(book: &'a Book, name: &str) -> AppResult<&'a Sheet> {
    book.sheet(name)
        .ok_or_else(|| AppError::invalid(format!("시트 '{name}' 이(가) 없습니다.")))
}

pub(super) fn header_or_err(sheet: &Sheet, needles: &[&str]) -> AppResult<u32> {
    sheet
        .find_header_row(needles, 8)
        .ok_or_else(|| AppError::invalid(format!("시트 '{}' 에서 머리글({})을 찾지 못했습니다.", sheet.name, needles.join("·"))))
}

pub(super) fn col_or_err(sheet: &Sheet, header_row: u32, name: &str) -> AppResult<u32> {
    sheet
        .col_of(header_row, name)
        .ok_or_else(|| AppError::invalid(format!("시트 '{}' 머리글에 '{name}' 열이 없습니다.", sheet.name)))
}

pub(super) fn student_at(sheet: &Sheet, row: u32, grade: Option<u32>, class: Option<u32>, no: Option<u32>, name: Option<u32>) -> StudentKey {
    let grade_text = grade.map(|c| sheet.text(row, c)).unwrap_or_default();
    let grade_val = grade_text.trim_end_matches("학년").trim().parse::<i32>().ok();
    StudentKey {
        grade: grade_val,
        class_name: class.map(|c| sheet.text(row, c)).unwrap_or_default(),
        student_no: no.map(|c| sheet.text(row, c)).unwrap_or_default(),
        name: name.map(|c| sheet.text(row, c)).unwrap_or_default(),
    }
}

/// 학생 칸 검사 — 이름 없음·학년 이상은 '확인 필요'.
pub(super) fn student_issues(s: &StudentKey) -> Vec<Issue> {
    let mut v = Vec::new();
    if s.name.is_empty() {
        v.push(Issue::check("학생 이름이 비어 있어 학생을 식별하지 못합니다."));
    }
    if let Some(g) = s.grade {
        if !(1..=6).contains(&g) {
            v.push(Issue::check(format!("학년 값이 이상합니다: {g}")));
        }
    }
    v
}

/// 금액 셀 읽기 — 오류는 Issue 로 바꾸고 None 을 돌려준다.
pub(super) fn read_amount(sheet: &Sheet, row: u32, col: u32, issues: &mut Vec<Issue>) -> Option<i64> {
    match amount_of(sheet.cell(row, col)) {
        Ok(v) => v,
        Err(e) => {
            issues.push(Issue::error(format!("{}열: {}", col_letter(col), e.message)));
            None
        }
    }
}

pub(super) fn col_letter(col: u32) -> String {
    let mut n = col;
    let mut s = String::new();
    while n > 0 {
        let r = (n - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

/// 합계 칸이 있으면 세부 합과 맞는지 본다.
pub(super) fn check_total(sheet: &Sheet, row: u32, total_col: Option<u32>, parts_sum: i64, issues: &mut Vec<Issue>) {
    let Some(tc) = total_col else { return };
    match amount_of(sheet.cell(row, tc)) {
        Ok(Some(t)) if t != parts_sum => issues.push(Issue::check(format!(
            "합계 칸({})과 세부 항목 합({})이 다릅니다.",
            crate::domain::money::format_won(t),
            crate::domain::money::format_won(parts_sum)
        ))),
        Ok(_) => {}
        // 합계 칸이 계산되지 않은 수식이면 0 으로 보지 않고 오류로 올린다.
        Err(e) => issues.push(Issue::error(format!("{}열(합계): {}", col_letter(tc), e.message))),
    }
}

pub const ITEM_HEADERS: [(&str, &str); 4] = [
    ("강사료", "instructor"),
    ("수용비", "admin"),
    ("교재비", "textbook"),
    ("재료비", "material"),
];
