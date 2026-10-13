//! Excel 읽기 (xlsx · xlsm). calamine 으로 값·수식·병합 영역을 읽는다.
//!
//! - xlsm 은 자료만 읽는다. 매크로는 실행되지 않는다(calamine 은 zip 안의 시트 XML 만 본다).
//! - 수식 셀은 **저장된 계산값**을 쓴다. 계산값이 없는 수식 셀은 0 으로 보지 않고
//!   `formula_without_value` 로 표시해 오류로 올린다.
//! - 셀 좌표는 엑셀 화면과 같은 1 기준 행·열이다.

pub mod detect;
pub mod parse;
pub mod formats;

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use calamine::{Data, Dimensions, Reader, Xlsx};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Empty,
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub value: CellValue,
    /// 수식이 있는데 저장된 값이 없다 (엑셀이 계산하지 않은 채 저장됨).
    pub formula_without_value: bool,
}

impl Cell {
    pub fn text(&self) -> String {
        match &self.value {
            CellValue::Text(s) => s.trim().to_string(),
            CellValue::Int(i) => i.to_string(),
            CellValue::Float(f) => {
                if f.fract() == 0.0 {
                    format!("{}", *f as i64)
                } else {
                    f.to_string()
                }
            }
            CellValue::Bool(b) => b.to_string(),
            CellValue::Error(e) => format!("#{e}"),
            CellValue::Empty => String::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.value, CellValue::Empty) && !self.formula_without_value
    }
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    /// 1 기준 행 번호 → (1 기준 열 번호 → 셀). 비어 있는 셀은 없다.
    cells: HashMap<(u32, u32), Cell>,
    pub max_row: u32,
    pub max_col: u32,
    /// 병합 영역 (1 기준, 양 끝 포함)
    pub merged: Vec<((u32, u32), (u32, u32))>,
}

static EMPTY: Cell = Cell {
    value: CellValue::Empty,
    formula_without_value: false,
};

impl Sheet {
    pub fn cell(&self, row: u32, col: u32) -> &Cell {
        self.cells.get(&(row, col)).unwrap_or(&EMPTY)
    }

    pub fn text(&self, row: u32, col: u32) -> String {
        self.cell(row, col).text()
    }

    /// 행에 값이 있는 셀이 하나라도 있는가 (지정한 열 범위 안에서).
    pub fn row_has_any(&self, row: u32, cols: std::ops::RangeInclusive<u32>) -> bool {
        cols.into_iter().any(|c| !self.cell(row, c).is_empty())
    }

    /// (row, col) 이 어떤 병합 영역의 **첫 칸이 아닌** 부분인가. 맞으면 영역의 첫 칸을 돌려준다.
    pub fn merged_origin(&self, row: u32, col: u32) -> Option<(u32, u32)> {
        self.merged.iter().find_map(|((r1, c1), (r2, c2))| {
            if row >= *r1 && row <= *r2 && col >= *c1 && col <= *c2 && (row, col) != (*r1, *c1) {
                Some((*r1, *c1))
            } else {
                None
            }
        })
    }

    /// 머리글 찾기 — `needles` 가 모두 들어 있는 첫 행 (앞 `max_scan` 행 안에서).
    pub fn find_header_row(&self, needles: &[&str], max_scan: u32) -> Option<u32> {
        (1..=self.max_row.min(max_scan)).find(|&r| {
            let texts: Vec<String> = (1..=self.max_col).map(|c| norm(&self.text(r, c))).collect();
            needles.iter().all(|n| texts.iter().any(|t| t == &norm(n)))
        })
    }

    /// 머리글 행에서 글자가 정확히 맞는 열 번호.
    pub fn col_of(&self, header_row: u32, name: &str) -> Option<u32> {
        (1..=self.max_col).find(|&c| norm(&self.text(header_row, c)) == norm(name))
    }
}

/// 머리글 비교용 정규화 — 공백·줄바꿈 제거.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

#[derive(Debug, Clone)]
pub struct Book {
    pub sheets: Vec<Sheet>,
}

impl Book {
    pub fn open(path: &Path) -> AppResult<Book> {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext != "xlsx" && ext != "xlsm" {
            return Err(AppError::invalid("xlsx 또는 xlsm 파일만 올릴 수 있습니다."));
        }
        let mut wb: Xlsx<BufReader<File>> = calamine::open_workbook(path).map_err(|e: calamine::XlsxError| {
            AppError::new("EXCEL_READ", "Excel 파일을 열지 못했습니다. 다른 프로그램에서 열려 있으면 닫아 주세요.")
                .detail(e.to_string())
        })?;
        wb.load_merged_regions().map_err(|e| {
            AppError::new("EXCEL_READ", "병합 셀 정보를 읽지 못했습니다.").detail(e.to_string())
        })?;
        let names: Vec<String> = wb.sheet_names().to_vec();
        let mut sheets = Vec::new();
        for name in names {
            let range = match wb.worksheet_range(&name) {
                Ok(r) => r,
                Err(calamine::XlsxError::NotAWorksheet(_)) => continue,
                Err(e) => {
                    return Err(AppError::new("EXCEL_READ", format!("시트 '{name}' 을(를) 읽지 못했습니다."))
                        .detail(e.to_string()))
                }
            };
            let formulas = wb.worksheet_formula(&name).unwrap_or_default();
            let mut cells = HashMap::new();
            let mut max_row = 0u32;
            let mut max_col = 0u32;
            let (r0, c0) = range.start().unwrap_or((0, 0));
            for (ri, row) in range.rows().enumerate() {
                for (ci, d) in row.iter().enumerate() {
                    let value = match d.clone() {
                        Data::Empty => CellValue::Empty,
                        Data::String(s) => {
                            if s.trim().is_empty() {
                                CellValue::Empty
                            } else {
                                CellValue::Text(s)
                            }
                        }
                        Data::Int(i) => CellValue::Int(i),
                        Data::Float(f) => CellValue::Float(f),
                        Data::Bool(b) => CellValue::Bool(b),
                        Data::Error(e) => CellValue::Error(format!("{e:?}")),
                        Data::DateTime(dt) => CellValue::Float(dt.as_f64()),
                        Data::DateTimeIso(s) | Data::DurationIso(s) => CellValue::Text(s),
                    };
                    if matches!(value, CellValue::Empty) {
                        continue;
                    }
                    let row_no = r0 + ri as u32 + 1;
                    let col_no = c0 + ci as u32 + 1;
                    max_row = max_row.max(row_no);
                    max_col = max_col.max(col_no);
                    cells.insert((row_no, col_no), Cell { value, formula_without_value: false });
                }
            }
            // 수식은 있는데 값이 없는 셀을 표시한다.
            let (fr0, fc0) = formulas.start().unwrap_or((0, 0));
            for (ri, row) in formulas.rows().enumerate() {
                for (ci, f) in row.iter().enumerate() {
                    if f.trim().is_empty() {
                        continue;
                    }
                    let key = (fr0 + ri as u32 + 1, fc0 + ci as u32 + 1);
                    if !cells.contains_key(&key) {
                        max_row = max_row.max(key.0);
                        max_col = max_col.max(key.1);
                        cells.insert(key, Cell { value: CellValue::Empty, formula_without_value: true });
                    }
                }
            }
            let merged = wb
                .merged_regions_by_sheet(&name)
                .into_iter()
                .map(|(_, _, d): (&String, &String, &Dimensions)| {
                    ((d.start.0 + 1, d.start.1 + 1), (d.end.0 + 1, d.end.1 + 1))
                })
                .collect();
            sheets.push(Sheet { name, cells, max_row, max_col, merged });
        }
        if sheets.is_empty() {
            return Err(AppError::invalid("읽을 수 있는 시트가 없습니다."));
        }
        Ok(Book { sheets })
    }

    pub fn sheet(&self, name: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.name == name)
    }
}

/// 금액 셀 → 원 단위 정수. `Ok(None)` 은 빈 칸(0 과 다르다).
pub fn amount_of(cell: &Cell) -> AppResult<Option<i64>> {
    if cell.formula_without_value {
        return Err(AppError::invalid("수식 셀에 저장된 계산값이 없습니다. 엑셀에서 열어 저장한 뒤 다시 올려 주세요."));
    }
    match &cell.value {
        CellValue::Empty => Ok(None),
        CellValue::Int(i) => Ok(Some(*i)),
        CellValue::Float(f) => {
            if f.fract() != 0.0 {
                return Err(AppError::invalid(format!("금액이 원 단위 정수가 아닙니다: {f}")));
            }
            if f.abs() > 9.0e15 {
                return Err(AppError::invalid("금액이 너무 큽니다."));
            }
            Ok(Some(*f as i64))
        }
        CellValue::Text(s) => {
            let t = s.trim();
            if t.is_empty() || t == "-" {
                return Ok(None);
            }
            Ok(Some(crate::domain::money::parse_won(t)?))
        }
        CellValue::Bool(_) => Err(AppError::invalid("금액 칸에 참/거짓 값이 있습니다.")),
        CellValue::Error(e) => Err(AppError::invalid(format!("금액 칸에 오류 값이 있습니다: {e}"))),
    }
}

pub fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../test/fixtures")
}

#[cfg(test)]
mod tests;
