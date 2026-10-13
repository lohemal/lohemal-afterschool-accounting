//! Excel 내보내기 (rust_xlsxwriter). 금액은 숫자, 날짜는 'YYYY-MM-DD' 문자열, 머리글 굵게, 합계 행 포함.
//! 파일에는 학생 개인정보가 들어갈 수 있다 — 화면에서 안내한다.

use std::path::Path;

use rusqlite::Connection;
use rust_xlsxwriter::{Format, Workbook, Worksheet};

use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::ops::history;
use crate::repo::{integrity, recon, voucher, year};

fn xerr(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::new("EXPORT", "Excel 파일을 쓰지 못했습니다.").detail(e.to_string())
}

struct Sheet<'a> {
    ws: &'a mut Worksheet,
    row: u32,
    head: Format,
    money: Format,
    bold_money: Format,
    bold: Format,
}

impl<'a> Sheet<'a> {
    fn new(ws: &'a mut Worksheet) -> Self {
        Sheet { ws, row: 0, head: Format::new().set_bold().set_background_color("DDE3EE"), money: Format::new().set_num_format("#,##0"), bold_money: Format::new().set_bold().set_num_format("#,##0"), bold: Format::new().set_bold() }
    }
    fn header(&mut self, cols: &[&str]) -> AppResult<()> {
        for (i, c) in cols.iter().enumerate() {
            self.ws.write_string_with_format(self.row, i as u16, *c, &self.head).map_err(xerr)?;
            self.ws.set_column_width(i as u16, (c.chars().count() as f64 * 2.2).max(10.0)).map_err(xerr)?;
        }
        self.row += 1;
        Ok(())
    }
    fn text(&mut self, col: u16, s: &str) -> AppResult<()> {
        self.ws.write_string(self.row, col, s).map_err(xerr)?;
        Ok(())
    }
    fn num(&mut self, col: u16, v: i64) -> AppResult<()> {
        self.ws.write_number_with_format(self.row, col, v as f64, &self.money).map_err(xerr)?;
        Ok(())
    }
    fn next(&mut self) {
        self.row += 1;
    }
    fn total_label(&mut self, col: u16, s: &str) -> AppResult<()> {
        self.ws.write_string_with_format(self.row, col, s, &self.bold).map_err(xerr)?;
        Ok(())
    }
    fn total_num(&mut self, col: u16, v: i64) -> AppResult<()> {
        self.ws.write_number_with_format(self.row, col, v as f64, &self.bold_money).map_err(xerr)?;
        Ok(())
    }
}

fn finish(wb: &mut Workbook, path: &Path, data_dir: &Path, sheet: &str, rows: i64, what: &str) -> AppResult<ExportResult> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    wb.save(path).map_err(xerr)?;
    let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    history::record(data_dir, "export", "ok", &format!("{what} 내보내기 {rows}행 → {name}"), Some(&name), serde_json::json!({ "sheet": sheet, "rows": rows, "path": path.display().to_string() }))?;
    Ok(ExportResult { path: path.display().to_string(), sheet: sheet.into(), rows })
}

pub fn today_tag() -> String {
    chrono::Local::now().format("%Y%m%d").to_string()
}

/// A. 월별 회계 검증 결과 — 화면 필터(상태·항목·프로그램)를 그대로 반영. 프로그램 행마다 재원별 줄을 쓴다.
pub fn recon_month(conn: &Connection, data_dir: &Path, fiscal_month_id: i64, filter: &ReconExportFilter, path: &Path) -> AppResult<ExportResult> {
    let r = recon::month(conn, fiscal_month_id)?;
    let m = year::month_label_of(conn, fiscal_month_id)?;
    let y = {
        let yid: i64 = conn.query_row("SELECT school_year_id FROM fiscal_month WHERE id = ?1", [fiscal_month_id], |x| x.get(0))?;
        year::get(conn, yid)?.name
    };
    let status = filter.status.clone().unwrap_or_else(|| "all".into());
    let attention = ["link_pending", "unallocated", "unmapped", "orphan", "mismatch", "missing"];
    let rows: Vec<&ReconRow> = r
        .rows
        .iter()
        .filter(|x| status == "all" || (status == "attention" && attention.contains(&x.status.as_str())) || x.status == status)
        .filter(|x| filter.item_set.as_deref().map(|i| i == "all" || i == x.item_set).unwrap_or(true))
        .filter(|x| filter.program_text.as_deref().map(|p| p.trim().is_empty() || x.unit_name.contains(p.trim())).unwrap_or(true))
        .collect();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("월별 검증").map_err(xerr)?;
    let mut s = Sheet::new(ws);
    s.header(&["학년도", "귀속월", "프로그램", "금액 항목", "재원", "징수액", "지급액", "차액", "검증 상태", "확인 메모"])?;
    let (mut tb, mut te, mut td) = (0i64, 0i64, 0i64);
    let mut n = 0;
    for x in &rows {
        let notes = recon::notes(conn, fiscal_month_id, &x.unit_kind, x.unit_id, &x.item_set)?;
        let memo = notes.first().map(|nn| format!("{} · {}", recon::note_label(&nn.status), nn.reason)).unwrap_or_default();
        let lines: Vec<&FundLine> = x.funds.iter().collect();
        if lines.is_empty() {
            s.text(0, &y)?;
            s.text(1, &m)?;
            s.text(2, &x.unit_name)?;
            s.text(3, &x.item_label)?;
            s.text(4, "—")?;
            s.num(5, x.basis)?;
            s.num(6, x.expense)?;
            s.num(7, x.simple_diff)?;
            s.text(8, &x.status_label)?;
            s.text(9, &memo)?;
            s.next();
            n += 1;
        }
        for (i, f) in lines.iter().enumerate() {
            s.text(0, &y)?;
            s.text(1, &m)?;
            s.text(2, &x.unit_name)?;
            s.text(3, &x.item_label)?;
            s.text(4, &f.fund_name)?;
            s.num(5, f.basis.unwrap_or(0))?;
            s.num(6, f.expense.unwrap_or(0))?;
            s.num(7, f.diff.unwrap_or(f.basis.unwrap_or(0) - f.expense.unwrap_or(0)))?;
            s.text(8, if i == 0 { &x.status_label } else { "" })?;
            s.text(9, if i == 0 { memo.as_str() } else { "" })?;
            s.next();
            n += 1;
        }
        // 통합 프로그램 행 아래 세부반 줄 (지급은 통합 행에만 — 중복 합산 없음)
        for ch in &x.children {
            s.text(0, &y)?;
            s.text(1, &m)?;
            s.text(2, &format!("  └ {}", ch.program_name))?;
            s.text(3, &x.item_label)?;
            s.text(4, if ch.direct { "직접 귀속" } else { "세부반" })?;
            s.num(5, ch.basis)?;
            s.num(6, ch.expense)?;
            s.text(7, "")?;
            s.text(8, "세부반 (통합 행에 합산됨)")?;
            s.text(9, "")?;
            s.next();
            n += 1;
        }
        tb += x.basis;
        te += x.expense;
        td += x.simple_diff;
    }
    s.total_label(0, "합계")?;
    s.total_label(2, &format!("{}개 프로그램·항목", rows.len()))?;
    s.total_num(5, tb)?;
    s.total_num(6, te)?;
    s.total_num(7, td)?;
    finish(&mut wb, path, data_dir, "월별 검증", n, "월별 회계 검증 결과")
}

/// B. 자유수강권 사용액 현황 — 학생마다 한 줄, 월별 사용액 12칸.
pub fn voucher_usage(conn: &Connection, data_dir: &Path, school_year_id: i64, path: &Path) -> AppResult<ExportResult> {
    let y = year::get(conn, school_year_id)?;
    let months = year::months(conn, school_year_id)?;
    let usages = voucher::usage_all(conn, school_year_id)?;
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("자유수강권 사용액").map_err(xerr)?;
    let mut s = Sheet::new(ws);
    let mut cols: Vec<String> = vec!["학년도", "학년", "반", "번호", "학생명", "대상자 상태", "지원 정책", "지원 한도", "이전 학교 사용액"].into_iter().map(String::from).collect();
    for mm in &months {
        cols.push(format!("{} 사용액", mm.label));
    }
    cols.extend(["누적 사용액", "지원 잔액", "수익자 전환액", "방과후이용권 사용액", "비고"].into_iter().map(String::from));
    let refs: Vec<&str> = cols.iter().map(|c| c.as_str()).collect();
    s.header(&refs)?;
    let mut totals = vec![0i64; months.len() + 4];
    for u in &usages {
        let (grade, class, no, name): (i64, String, String, String) = conn.query_row("SELECT grade, class_name, student_no, name FROM student WHERE id = ?1", [u.student_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        s.text(0, &y.name)?;
        s.num(1, grade)?;
        s.text(2, &class)?;
        s.text(3, &no)?;
        s.text(4, &name)?;
        s.text(5, &u.target_status_label)?;
        s.text(6, &format!("{} ({})", u.basis_label, u.limit_source))?;
        match u.limit_total {
            Some(v) => s.num(7, v)?,
            None => s.text(7, "제한 없음")?,
        }
        match u.prior_used {
            Some(v) => s.num(8, v)?,
            None => s.text(8, "")?,
        }
        for (i, mm) in months.iter().enumerate() {
            let v: i64 = u.rows.iter().filter(|r| r.fiscal_month_id == mm.id).map(|r| r.free_actual).sum();
            s.num(9 + i as u16, v)?;
            totals[i] += v;
        }
        let base = 9 + months.len() as u16;
        let conv: i64 = u.rows.iter().filter(|r| r.in_window).map(|r| r.beneficiary_expected).sum();
        s.num(base, u.used_actual)?;
        match u.remaining {
            Some(v) => s.num(base + 1, v)?,
            None => s.text(base + 1, "제한 없음")?,
        }
        s.num(base + 2, conv)?;
        s.num(base + 3, u.voucher_total)?;
        s.text(base + 4, &[u.blocked.clone(), u.warnings.clone()].concat().join(" / "))?;
        totals[months.len()] += u.used_actual;
        totals[months.len() + 1] += u.remaining.unwrap_or(0);
        totals[months.len() + 2] += conv;
        totals[months.len() + 3] += u.voucher_total;
        s.next();
    }
    s.total_label(0, "합계")?;
    s.total_label(4, &format!("{}명", usages.len()))?;
    for (i, v) in totals.iter().enumerate() {
        s.total_num(9 + i as u16, *v)?;
    }
    finish(&mut wb, path, data_dir, "자유수강권 사용액", usages.len() as i64, "자유수강권 사용액 현황")
}

/// C. 자유수강권 소급 환불 현황 — 화면 필터(상태·기존 재원) 반영.
pub fn voucher_refunds(conn: &Connection, data_dir: &Path, school_year_id: i64, filter: &RefundExportFilter, path: &Path) -> AppResult<ExportResult> {
    let y = year::get(conn, school_year_id)?;
    let status = filter.status.clone().unwrap_or_else(|| "all".into());
    let rows: Vec<VoucherRefundRow> = voucher::refund_list(conn, school_year_id)?
        .into_iter()
        .filter(|r| status == "all" || (status == "open" && r.status != "complete" && r.status != "cancelled") || r.status == status)
        .filter(|r| filter.fund_name.as_deref().map(|f| f == "all" || f == r.from_fund_name).unwrap_or(true))
        .collect();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("소급 환불").map_err(xerr)?;
    let mut s = Sheet::new(ws);
    s.header(&["학년도", "학생", "귀속월", "프로그램", "항목", "기존 재원", "재원경정액", "환불 예정액", "환불 완료액", "미환불액", "처리 상태", "처리일", "경정 번호"])?;
    let (mut a, mut b, mut c, mut d) = (0i64, 0i64, 0i64, 0i64);
    for r in &rows {
        s.text(0, &y.name)?;
        s.text(1, &r.student)?;
        s.text(2, &r.month_label)?;
        s.text(3, &r.program_name)?;
        s.text(4, &r.item_name)?;
        s.text(5, &r.from_fund_name)?;
        s.num(6, r.amount)?;
        s.num(7, r.returned_planned)?;
        s.num(8, r.returned_done)?;
        s.num(9, r.outstanding)?;
        s.text(10, &r.status_label)?;
        s.text(11, r.returns.iter().filter(|x| x.status == "done").filter_map(|x| x.processed_on.clone()).max().as_deref().unwrap_or(""))?;
        s.num(12, r.transfer_id)?;
        a += r.amount;
        b += r.returned_planned;
        c += r.returned_done;
        d += r.outstanding;
        s.next();
    }
    s.total_label(0, "합계")?;
    s.total_label(1, &format!("{}건", rows.len()))?;
    s.total_num(6, a)?;
    s.total_num(7, b)?;
    s.total_num(8, c)?;
    s.total_num(9, d)?;
    finish(&mut wb, path, data_dir, "소급 환불", rows.len() as i64, "자유수강권 소급 환불 현황")
}

/// D. 회계 무결성 검사 결과.
pub fn integrity_report(conn: &Connection, data_dir: &Path, school_year_id: i64, path: &Path) -> AppResult<ExportResult> {
    let rep = integrity::run(conn, data_dir, school_year_id)?;
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("무결성 검사").map_err(xerr)?;
    let mut s = Sheet::new(ws);
    s.header(&["검사일", "검사 항목", "결과 등급", "내용", "학생", "프로그램", "귀속월", "관련 거래", "예상 원인", "확인 방법", "수정 안내"])?;
    for it in &rep.items {
        s.text(0, &rep.checked_at)?;
        s.text(1, &format!("{} ({})", it.title, it.code))?;
        s.text(2, integrity::level_label(&it.level))?;
        s.text(3, &it.detail)?;
        s.text(4, &it.student)?;
        s.text(5, &it.program)?;
        s.text(6, &it.month)?;
        s.text(7, &it.entry_ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", "))?;
        s.text(8, &it.cause)?;
        s.text(9, &it.how_to_check)?;
        s.text(10, &it.fix_hint)?;
        s.next();
    }
    s.total_label(0, "요약")?;
    s.total_label(1, &format!("정상 {} · 확인 필요 {} · 오류 {}", rep.ok_count, rep.check_count, rep.error_count))?;
    s.next();
    let ws2 = wb.add_worksheet();
    ws2.set_name("검사 항목 요약").map_err(xerr)?;
    let mut s2 = Sheet::new(ws2);
    s2.header(&["검사 항목", "코드", "등급", "건수"])?;
    for c in &rep.checks {
        s2.text(0, &c.title)?;
        s2.text(1, &c.code)?;
        s2.text(2, integrity::level_label(&c.level))?;
        s2.num(3, c.count)?;
        s2.next();
    }
    finish(&mut wb, path, data_dir, "무결성 검사", rep.items.len() as i64, "회계 무결성 검사 결과")
}
