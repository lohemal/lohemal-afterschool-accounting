//! 업로드 파일 · 정규화 행 · 적용/취소/복원.
//!
//! 흐름
//!   inspect  파일을 열어 양식을 인식한다 (DB 에 아무것도 쓰지 않는다)
//!   preview  해석 + 프로그램·재원 해석 + 검사 → 화면에 보여줄 미리보기. 역시 쓰지 않는다
//!   commit   한 트랜잭션: 파일 등록 → 원본 행 → 정규화 행 → 거래 → 합계 검산
//!   cancel   파일의 살아 있는 거래를 전부 취소(void). 다른 파일의 거래는 건드리지 않는다
//!   restore  계보 안의 이전 판을 다시 적용한다. 현재 판은 취소하고, 이전 판의 저장된
//!            정규화 행에서 **새 거래**를 만든다 → 원장 UPDATE·DELETE 없음, 중복 집계 없음
//!
//! 원본 파일은 앱 자료 폴더 `uploads/<sha256>.<확장자>` 에 보관한다(내용 주소라 복사가 멱등).
//! DB 트랜잭션과 파일 복사는 별개이므로 복사를 먼저 하고, 시작할 때 고아 파일을 치운다.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::json;

use crate::domain::{hash, money};
use crate::error::{AppError, AppResult};
use crate::excel::detect::{self, FormatCode};
use crate::excel::parse::{self, Issue, ParsedRow, SourceRow, StudentKey};
use crate::excel::Book;
use crate::model::*;
use crate::repo::program::Target;
use crate::repo::{audit, catalog, fundalias, ledger, program, student, year};

// ── 자료 유형 ────────────────────────────────────────────────────────────

pub const KINDS: [(&str, &str); 6] = [
    ("initial_collect", "최초 징수"),
    ("extra_refund", "추가징수·환불"),
    ("instructor_pay", "강사료 지급"),
    ("vendor_pay", "교재·재료비 지급"),
    ("roster_link", "프로그램 연결·검증"),
    ("snapshot", "참고 스냅샷"),
];

pub fn kind_label(k: &str) -> String {
    KINDS.iter().find(|(c, _)| *c == k).map(|(_, l)| l.to_string()).unwrap_or_else(|| k.to_string())
}

/// 원장에 거래를 만드는 유형인가.
pub fn is_ledger_kind(k: &str) -> bool {
    matches!(k, "initial_collect" | "extra_refund" | "instructor_pay" | "vendor_pay")
}

/// 같은 귀속월에 적용본이 하나만 있어야 하는 유형 (두 번 올리면 두 배로 잡힌다).
fn exclusive_kind(k: &str) -> bool {
    matches!(k, "initial_collect" | "instructor_pay" | "vendor_pay")
}

pub fn status_label(s: &str) -> &'static str {
    match s {
        "registered" => "등록",
        "previewed" => "미리보기",
        "applied" => "적용 중",
        "replaced" => "이전 버전",
        "cancelled" => "취소됨",
        _ => "알 수 없음",
    }
}

// ── 파일 목록 ────────────────────────────────────────────────────────────

const COLS: &str = "u.id, u.school_year_id, u.file_name, u.sha256, u.byte_size, u.format_code, u.version_no,
    u.status, u.replaced_by_id, u.note, u.uploaded_at, u.applied_at, u.cancelled_at,
    (SELECT count(*) FROM upload_row r WHERE r.upload_file_id = u.id),
    (SELECT count(*) FROM ledger_entry e WHERE e.upload_file_id = u.id AND e.state = 'active'),
    u.data_kind, u.fiscal_month_id, m.ym, u.lineage_id, u.stored_name, u.summary_json,
    (SELECT coalesce(sum(e.amount), 0) FROM ledger_entry e WHERE e.upload_file_id = u.id AND e.state = 'active' AND e.flow = 'collection'),
    (SELECT coalesce(sum(e.amount), 0) FROM ledger_entry e WHERE e.upload_file_id = u.id AND e.state = 'active' AND e.flow = 'expenditure')";

const FROM: &str = "FROM upload_file u LEFT JOIN fiscal_month m ON m.id = u.fiscal_month_id";

fn map(r: &Row) -> rusqlite::Result<UploadFile> {
    let format_code: String = r.get(5)?;
    let status: String = r.get(7)?;
    let data_kind: String = r.get(15)?;
    let ym: Option<String> = r.get(17)?;
    let summary_json: String = r.get(20)?;
    Ok(UploadFile {
        id: r.get(0)?,
        school_year_id: r.get(1)?,
        file_name: r.get(2)?,
        sha256: r.get(3)?,
        byte_size: r.get(4)?,
        format_label: FormatCode::from_code(&format_code).map(|f| f.label().to_string()).unwrap_or_else(|| format_code.clone()),
        format_code,
        data_kind_label: kind_label(&data_kind),
        data_kind,
        fiscal_month_id: r.get(16)?,
        fiscal_month_label: ym.map(|y| format!("{} ({y})", crate::domain::months::label(&y))).unwrap_or_default(),
        lineage_id: r.get(18)?,
        version_no: r.get(6)?,
        status_label: status_label(&status).to_string(),
        status,
        replaced_by_id: r.get(8)?,
        stored_name: r.get(19)?,
        note: r.get(9)?,
        uploaded_at: r.get(10)?,
        applied_at: r.get(11)?,
        cancelled_at: r.get(12)?,
        row_count: r.get(13)?,
        entry_count: r.get(14)?,
        active_collection_total: r.get(21)?,
        active_expenditure_total: r.get(22)?,
        summary: serde_json::from_str(&summary_json).unwrap_or_default(),
    })
}

pub fn list(conn: &Connection, school_year_id: i64) -> AppResult<Vec<UploadFile>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} {FROM} WHERE u.school_year_id = ?1 ORDER BY u.id DESC"))?;
    let rows = st.query_map([school_year_id], map)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<UploadFile> {
    conn.query_row(&format!("SELECT {COLS} {FROM} WHERE u.id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("업로드 파일을 찾지 못했습니다."))
}

pub fn lineage(conn: &Connection, file_id: i64) -> AppResult<Vec<UploadFile>> {
    let f = get(conn, file_id)?;
    let lid = f.lineage_id.unwrap_or(f.id);
    let mut st = conn.prepare(&format!("SELECT {COLS} {FROM} WHERE u.lineage_id = ?1 OR u.id = ?1 ORDER BY u.version_no"))?;
    let rows = st.query_map([lid], map)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 살아 있는(취소·대체되지 않은) 같은 내용의 파일 — 중복 감지.
pub fn find_live_by_sha(conn: &Connection, school_year_id: i64, sha256: &str) -> AppResult<Option<UploadFile>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} {FROM} WHERE u.school_year_id = ?1 AND u.sha256 = ?2 AND u.status NOT IN ('cancelled', 'replaced')"),
            params![school_year_id, sha256],
            map,
        )
        .optional()?)
}

/// 같은 귀속월·같은 자료 유형으로 적용 중인 파일.
pub fn find_applied(conn: &Connection, fiscal_month_id: i64, data_kind: &str) -> AppResult<Option<UploadFile>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} {FROM} WHERE u.fiscal_month_id = ?1 AND u.data_kind = ?2 AND u.status = 'applied' ORDER BY u.id DESC LIMIT 1"),
            params![fiscal_month_id, data_kind],
            map,
        )
        .optional()?)
}

// ── 파일 보관 ────────────────────────────────────────────────────────────

pub fn uploads_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("uploads")
}

/// 원본을 앱 자료 폴더에 보관한다. 임시 파일에 쓰고 원자적으로 옮긴다. 같은 내용이면 그대로 둔다.
pub fn store_file(data_dir: &Path, src: &Path, sha256: &str) -> AppResult<String> {
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "xlsx".into());
    let name = format!("{sha256}.{ext}");
    let dir = uploads_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(&name);
    if dest.exists() {
        return Ok(name);
    }
    let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
    std::fs::copy(src, &tmp)?;
    // 복사된 내용이 맞는지 다시 확인한다.
    let (sha_after, _) = hash::sha256_file(&tmp)?;
    if sha_after != sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::new("IO", "파일을 보관하는 동안 내용이 바뀌었습니다. 다시 시도해 주세요."));
    }
    std::fs::rename(&tmp, &dest).or_else(|_| {
        // 다른 쪽이 먼저 같은 내용을 놓았으면 그대로 쓴다.
        if dest.exists() {
            let _ = std::fs::remove_file(&tmp);
            Ok(())
        } else {
            Err(std::io::Error::other("rename failed"))
        }
    })?;
    Ok(name)
}

/// DB 에 없는 보관 파일(앱이 반영 도중 꺼진 경우 등)을 치운다. 시작할 때 부른다.
pub fn cleanup_orphans(conn: &Connection, data_dir: &Path) -> AppResult<usize> {
    let dir = uploads_dir(data_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(0) };
    let mut st = conn.prepare("SELECT DISTINCT stored_name FROM upload_file WHERE stored_name <> ''")?;
    let known: HashSet<String> = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    let mut removed = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !known.contains(&name) {
            if std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        log::info!("고아 보관 파일 {removed}개 정리");
    }
    Ok(removed)
}

// ── 1. 인식 ──────────────────────────────────────────────────────────────

pub fn inspect(conn: &Connection, school_year_id: i64, path: &Path) -> AppResult<Inspect> {
    if !path.is_file() {
        return Err(AppError::invalid("파일을 찾지 못했습니다."));
    }
    let book = Book::open(path)?;
    let (sha256, size) = hash::sha256_file(path)?;
    let detected = detect::detect(&book);
    Ok(Inspect {
        path: path.display().to_string(),
        file_name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        byte_size: size as i64,
        sha256: sha256.clone(),
        sheets: book.sheets.iter().map(|s| SheetInfo { name: s.name.clone(), max_row: s.max_row, max_col: s.max_col }).collect(),
        detected,
        suggested_month: detect::suggest_month(&book),
        duplicate_of: find_live_by_sha(conn, school_year_id, &sha256)?,
    })
}

// ── 2. 미리보기 (해석 + 검사) ─────────────────────────────────────────────

/// 해석된 행 — 미리보기와 반영이 같은 것을 본다.
#[derive(Debug, Clone)]
struct Resolved {
    row: ParsedRow,
    program_id: Option<i64>,
    program_group_id: Option<i64>,
    program_name: String,
    fund_type_id: Option<i64>,
    fund_name: String,
    item_name: String,
    status: String,
    included: bool,
    issues: Vec<Issue>,
}

struct Built {
    parsed: parse::Parsed,
    resolved: Vec<Resolved>,
    preview: Preview,
    format: FormatCode,
}

fn student_label(s: &Option<StudentKey>) -> String {
    match s {
        None => String::new(),
        Some(k) => {
            let mut parts = Vec::new();
            if let Some(g) = k.grade {
                parts.push(format!("{g}학년"));
            }
            if !k.class_name.is_empty() {
                parts.push(format!("{}반", k.class_name));
            }
            if !k.student_no.is_empty() {
                parts.push(format!("{}번", k.student_no));
            }
            if !k.name.is_empty() {
                parts.push(k.name.clone());
            }
            parts.join(" ")
        }
    }
}

fn build(conn: &Connection, req: &PreviewRequest) -> AppResult<Built> {
    let path = Path::new(&req.path);
    let format = FormatCode::from_code(&req.format_code)
        .ok_or_else(|| AppError::invalid(format!("알 수 없는 양식: {}", req.format_code)))?;
    if !format.allowed_kinds().contains(&req.data_kind.as_str()) {
        return Err(AppError::invalid(format!(
            "'{}' 양식은 '{}' 자료 유형으로 올릴 수 없습니다.",
            format.label(),
            kind_label(&req.data_kind)
        )));
    }
    let month_year: Option<i64> = conn
        .query_row("SELECT school_year_id FROM fiscal_month WHERE id = ?1", [req.fiscal_month_id], |r| r.get(0))
        .optional()?;
    if month_year != Some(req.school_year_id) {
        return Err(AppError::invalid("귀속월이 학년도에 속하지 않습니다."));
    }
    let book = Book::open(path)?;
    let parsed = parse::parse(&book, format)?;
    let ledger_kind = is_ledger_kind(&req.data_kind);
    let format_has_program = format != FormatCode::CollectInitial;

    let items = catalog::amount_items(conn)?;
    let funds = catalog::fund_types(conn)?;
    let fund_name = |id: Option<i64>| -> String {
        id.and_then(|i| funds.iter().find(|f| f.id == i)).map(|f| f.name.clone()).unwrap_or_else(|| "미확정".into())
    };
    let overrides: HashMap<(String, u32), &RowOverride> =
        req.overrides.iter().map(|o| ((o.sheet.clone(), o.row_no), o)).collect();

    let mut program_cache: HashMap<String, Option<Target>> = HashMap::new();
    let mut fund_cache: HashMap<String, Option<i64>> = HashMap::new();
    let mut unresolved_programs: BTreeMap<String, i64> = BTreeMap::new();
    let mut unresolved_funds: BTreeMap<String, i64> = BTreeMap::new();
    let mut resolved: Vec<Resolved> = Vec::with_capacity(parsed.rows.len());

    for row in &parsed.rows {
        let mut issues = row.issues.clone();
        // 프로그램
        let mut program_id = None;
        let mut program_group_id = None;
        let mut program_name = String::new();
        if let Some(o) = overrides.get(&(row.sheet.clone(), row.row_no)) {
            if let Some(p) = o.program_id {
                program_id = Some(p);
                program_name = program::name_of(conn, p)?;
                issues.push(Issue::info(format!("사용자가 '{program_name}' 으로 지정")));
            } else if let Some(g) = o.program_group_id {
                program_group_id = Some(g);
                program_name = format!("{} (그룹)", program::group_name_of(conn, g)?);
                issues.push(Issue::info(format!("사용자가 '{program_name}' 으로 지정")));
            }
        }
        if program_id.is_none() && program_group_id.is_none() && !row.program_text.is_empty() {
            let t = row.program_text.clone();
            let target = match program_cache.get(&t) {
                Some(v) => *v,
                None => {
                    let v = program::resolve_target(conn, req.school_year_id, &t)?;
                    program_cache.insert(t.clone(), v);
                    v
                }
            };
            match target {
                Some(Target::Program(p)) => {
                    program_id = Some(p);
                    program_name = program::name_of(conn, p)?;
                }
                Some(Target::Group(g)) => {
                    program_group_id = Some(g);
                    program_name = format!("{} (그룹)", program::group_name_of(conn, g)?);
                }
                None => {
                    *unresolved_programs.entry(t.clone()).or_default() += 1;
                    issues.push(Issue::check(format!("프로그램 '{t}' 이(가) 등록되어 있지 않습니다. 매핑을 확정해 주세요.")));
                }
            }
        }
        if program_id.is_none() && program_group_id.is_none() && row.program_text.is_empty() && format_has_program && ledger_kind {
            if !issues.iter().any(|i| i.level == "check" && i.message.contains("부서")) {
                issues.push(Issue::check("프로그램을 지정해 주세요."));
            }
        }
        // 재원
        let mut fund_type_id: Option<i64> = None;
        if !row.fund_code.is_empty() {
            fund_type_id = Some(fundalias::effective_id(conn, catalog::fund_type_by_code(conn, &row.fund_code)?.id)?);
        } else if !row.fund_text.is_empty() {
            let t = row.fund_text.clone();
            let v = match fund_cache.get(&t) {
                Some(v) => *v,
                None => {
                    let v = fundalias::resolve(conn, &t)?;
                    fund_cache.insert(t.clone(), v);
                    v
                }
            };
            fund_type_id = v;
            if v.is_none() && ledger_kind {
                *unresolved_funds.entry(t.clone()).or_default() += 1;
                issues.push(Issue::check(format!("재원 '{t}' 을(를) 알 수 없어 '미확정'으로 등록됩니다. 설정에서 별칭을 추가할 수 있습니다.")));
            }
        }
        let item_name = items.iter().find(|i| i.code == row.item_code).map(|i| i.name.clone()).unwrap_or_default();
        if ledger_kind && !row.entry_kind.is_empty() && item_name.is_empty() {
            issues.push(Issue::error(format!("금액 항목 '{}' 이(가) 없습니다.", row.item_code)));
        }
        let status = if issues.iter().any(|i| i.level == "error") {
            "error"
        } else if issues.iter().any(|i| i.level == "check") {
            "check"
        } else {
            "ok"
        };
        let included = ledger_kind && !row.entry_kind.is_empty() && row.amount != 0 && status != "error";
        resolved.push(Resolved {
            row: row.clone(),
            program_id,
            program_group_id,
            program_name,
            fund_type_id,
            fund_name: fund_name(fund_type_id),
            item_name,
            status: status.to_string(),
            included,
            issues,
        });
    }

    // 요약
    let summary = summarize(&parsed, &resolved);

    // 반영 가능 여부
    let mut block_reasons = Vec::new();
    let mut warnings = Vec::new();
    if ledger_kind {
        if summary.error_count > 0 {
            block_reasons.push(format!("오류 행이 {}건 있습니다. 엑셀을 고친 뒤 다시 올려 주세요.", summary.error_count));
        }
        if !unresolved_programs.is_empty() {
            block_reasons.push(format!("등록되지 않은 프로그램 표기가 {}개 있습니다. 매핑을 확정해 주세요.", unresolved_programs.len()));
        }
        let blank_program = resolved
            .iter()
            .filter(|r| r.included && r.program_id.is_none() && r.program_group_id.is_none() && format_has_program)
            .count();
        if blank_program > 0 {
            block_reasons.push(format!("프로그램이 비어 있는 행이 {blank_program}건 있습니다. 행마다 프로그램을 지정해 주세요."));
        }
        if summary.included_count == 0 && block_reasons.is_empty() {
            block_reasons.push("원장에 넣을 금액 행이 없습니다.".into());
        }
        if !unresolved_funds.is_empty() {
            warnings.push(format!("재원을 알 수 없는 표기가 {}개 있어 '미확정'으로 등록됩니다.", unresolved_funds.len()));
        }
    }
    if year::month_is_closed(conn, req.fiscal_month_id)? {
        block_reasons.push("확정된 달입니다. 월별 정산에서 재개한 뒤 올려 주세요.".into());
    }
    let (sha256, _) = hash::sha256_file(path)?;
    if let Some(dup) = find_live_by_sha(conn, req.school_year_id, &sha256)? {
        block_reasons.push(format!("같은 내용의 파일이 이미 등록되어 있습니다: '{}' (제{}판, {})", dup.file_name, dup.version_no, dup.status_label));
    }
    let mut existing_applied = None;
    if ledger_kind && exclusive_kind(&req.data_kind) {
        if let Some(ex) = find_applied(conn, req.fiscal_month_id, &req.data_kind)? {
            if req.replace_file_id != Some(ex.id) {
                block_reasons.push(format!(
                    "이 달에 이미 적용 중인 {} 파일이 있습니다: '{}' (제{}판). 두 번 반영하면 금액이 두 배가 되므로 '교체'로 올려 주세요.",
                    ex.data_kind_label, ex.file_name, ex.version_no
                ));
            }
            existing_applied = Some(ex);
        }
    }
    if let Some(rid) = req.replace_file_id {
        let old = get(conn, rid)?;
        if old.status != "applied" {
            block_reasons.push("교체 대상이 적용 중인 파일이 아닙니다.".into());
        }
        if old.fiscal_month_id != Some(req.fiscal_month_id) || old.data_kind != req.data_kind {
            block_reasons.push("교체 대상과 귀속월·자료 유형이 다릅니다.".into());
        }
    }
    for fi in &parsed.file_issues {
        if fi.level == "check" {
            warnings.push(fi.message.clone());
        }
    }

    let mut unresolved: Vec<UnresolvedProgram> = Vec::new();
    for (text, count) in &unresolved_programs {
        let s = program::suggest(conn, req.school_year_id, text)?;
        unresolved.push(UnresolvedProgram {
            text: text.clone(),
            count: *count,
            suggestion_program_id: s.as_ref().map(|x| x.0),
            suggestion_name: s.map(|x| x.1).unwrap_or_default(),
        });
    }
    let rows: Vec<PreviewRow> = resolved.iter().map(to_preview_row).collect();
    let preview = Preview {
        format_code: format.code().into(),
        format_label: format.label().into(),
        data_kind: req.data_kind.clone(),
        data_kind_label: kind_label(&req.data_kind),
        summary,
        rows,
        unresolved_programs: unresolved,
        unresolved_funds: unresolved_funds.into_iter().map(|(text, count)| UnresolvedFund { text, count }).collect(),
        file_issues: parsed.file_issues.clone(),
        can_commit: block_reasons.is_empty(),
        block_reasons,
        warnings,
        existing_applied,
    };
    Ok(Built { parsed, resolved, preview, format })
}

fn to_preview_row(r: &Resolved) -> PreviewRow {
    PreviewRow {
        seq: r.row.seq,
        sheet: r.row.sheet.clone(),
        row_no: r.row.row_no,
        program_text: r.row.program_text.clone(),
        program_name: r.program_name.clone(),
        program_suggestion: r.row.program_suggestion.clone(),
        student: student_label(&r.row.student),
        fund_text: r.row.fund_text.clone(),
        fund_name: if r.row.fund_text.is_empty() && r.row.fund_code.is_empty() && r.fund_type_id.is_none() {
            "미확정".into()
        } else {
            r.fund_name.clone()
        },
        item_name: r.item_name.clone(),
        entry_kind: r.row.entry_kind.clone(),
        amount: r.row.amount,
        counterparty: r.row.counterparty.clone(),
        status: r.status.clone(),
        included: r.included,
        issues: r.issues.clone(),
    }
}

fn summarize(parsed: &parse::Parsed, resolved: &[Resolved]) -> UploadSummary {
    let mut by_item: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut by_fund: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut by_program: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut s = UploadSummary {
        source_row_count: parsed.source_rows.len() as i64,
        data_row_count: parsed.data_row_count as i64,
        excluded_row_count: parsed.excluded_row_count as i64,
        row_count: resolved.len() as i64,
        ..Default::default()
    };
    for r in resolved {
        match r.status.as_str() {
            "ok" => s.ok_count += 1,
            "check" => s.check_count += 1,
            _ => s.error_count += 1,
        }
        if !r.included {
            continue;
        }
        s.included_count += 1;
        match r.row.flow.as_str() {
            "collection" => s.collection_total += r.row.amount,
            "expenditure" => s.expenditure_total += r.row.amount,
            _ => {}
        }
        let bump = |m: &mut BTreeMap<String, (i64, i64)>, k: String| {
            let e = m.entry(k).or_default();
            e.0 += 1;
            e.1 += r.row.amount;
        };
        bump(&mut by_item, r.item_name.clone());
        bump(&mut by_fund, r.fund_name.clone());
        bump(&mut by_program, if r.program_name.is_empty() { "미배분".into() } else { r.program_name.clone() });
    }
    let lines = |m: BTreeMap<String, (i64, i64)>| m.into_iter().map(|(name, (count, total))| SummaryLine { name, count, total }).collect();
    s.by_item = lines(by_item);
    s.by_fund = lines(by_fund);
    s.by_program = lines(by_program);
    s
}

pub fn preview(conn: &Connection, req: &PreviewRequest) -> AppResult<Preview> {
    Ok(build(conn, req)?.preview)
}

// ── 3. 반영 (한 트랜잭션) ──────────────────────────────────────────────────

/// 파일 복사는 트랜잭션 바깥에서 먼저 한다(내용 주소라 실패해도 고아 파일만 남고, 시작 때 치운다).
pub fn commit(tx: &Connection, data_dir: &Path, req: &PreviewRequest) -> AppResult<CommitResult> {
    let built = build(tx, req)?;
    if !built.preview.can_commit {
        return Err(AppError::invalid(format!("반영할 수 없습니다: {}", built.preview.block_reasons.join(" / "))));
    }
    let path = Path::new(&req.path);
    let (sha256, size) = hash::sha256_file(path)?;
    let stored_name = store_file(data_dir, path, &sha256)?;
    let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

    // 교체: 옛 판의 거래를 먼저 취소한다.
    let (lineage_id, version_no) = match req.replace_file_id {
        Some(old_id) => {
            let old = get(tx, old_id)?;
            let voided = void_file_entries(tx, old_id, &format!("'{file_name}' 으로 교체"))?;
            tx.execute(
                "UPDATE upload_file SET status = 'replaced', cancelled_at = datetime('now', 'localtime') WHERE id = ?1",
                [old_id],
            )?;
            record_apply(tx, old_id, "replace", voided.0, voided.1, voided.2, &format!("'{file_name}' 으로 교체됨"))?;
            let lid = old.lineage_id.unwrap_or(old.id);
            let v: i64 = tx.query_row(
                "SELECT coalesce(max(version_no), 0) + 1 FROM upload_file WHERE lineage_id = ?1 OR id = ?1",
                [lid],
                |r| r.get(0),
            )?;
            (Some(lid), v)
        }
        None => (None, 1),
    };
    let status = if is_ledger_kind(&req.data_kind) { "applied" } else { "applied" };
    tx.execute(
        "INSERT INTO upload_file (school_year_id, file_name, sha256, byte_size, format_code, version_no, status, note,
                                  data_kind, fiscal_month_id, lineage_id, stored_name, original_path, summary_json, applied_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, datetime('now', 'localtime'))",
        params![
            req.school_year_id,
            file_name,
            sha256,
            size as i64,
            built.format.code(),
            version_no,
            status,
            req.note.clone().unwrap_or_default(),
            req.data_kind,
            req.fiscal_month_id,
            lineage_id,
            stored_name,
            req.path,
            serde_json::to_string(&built.preview.summary)?,
        ],
    )?;
    let file_id = tx.last_insert_rowid();
    if lineage_id.is_none() {
        tx.execute("UPDATE upload_file SET lineage_id = ?1 WHERE id = ?1", [file_id])?;
    }
    if let Some(old_id) = req.replace_file_id {
        tx.execute("UPDATE upload_file SET replaced_by_id = ?2 WHERE id = ?1", params![old_id, file_id])?;
    }

    // 원본 행
    let mut source_ids: HashMap<(String, u32), i64> = HashMap::new();
    for sr in &built.parsed.source_rows {
        let cells = json!(sr.cells).to_string();
        let row_hash = hash::sha256_hex(cells.as_bytes());
        tx.execute(
            "INSERT INTO source_row (upload_file_id, sheet_name, row_no, cells_json, row_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![file_id, sr.sheet, sr.row_no, cells, row_hash],
        )?;
        source_ids.insert((sr.sheet.clone(), sr.row_no), tx.last_insert_rowid());
    }

    // 정규화 행 + 거래
    let items = catalog::amount_items(tx)?;
    let mut entry_count = 0i64;
    let mut expect_coll = 0i64;
    let mut expect_exp = 0i64;
    for r in &built.resolved {
        let student_id = match &r.row.student {
            Some(k) => student::upsert(tx, req.school_year_id, k)?,
            None => None,
        };
        let src = source_ids.get(&(r.row.sheet.clone(), r.row.row_no)).copied();
        let (grade, class_name, student_no, student_name) = match &r.row.student {
            Some(k) => (k.grade, k.class_name.clone(), k.student_no.clone(), k.name.clone()),
            None => (None, String::new(), String::new(), String::new()),
        };
        tx.execute(
            "INSERT INTO upload_row (upload_file_id, source_row_id, seq, sheet_name, row_no, program_text, program_id, program_group_id,
                                     grade, class_name, student_no, student_name, student_id, fund_text, fund_type_id, amount_item_code,
                                     entry_kind, flow, amount, counterparty, status, included, issues_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
            params![
                file_id, src, r.row.seq, r.row.sheet, r.row.row_no, r.row.program_text, r.program_id, r.program_group_id,
                grade, class_name, student_no, student_name, student_id,
                if r.row.fund_code.is_empty() { r.row.fund_text.clone() } else { r.row.fund_code.clone() },
                r.fund_type_id, r.row.item_code, r.row.entry_kind, r.row.flow, r.row.amount, r.row.counterparty,
                r.status, r.included as i64, serde_json::to_string(&r.issues)?
            ],
        )?;
        if !r.included {
            continue;
        }
        let item = items
            .iter()
            .find(|i| i.code == r.row.item_code)
            .ok_or_else(|| AppError::invalid(format!("금액 항목 '{}' 이(가) 없습니다.", r.row.item_code)))?;
        ledger::insert(
            tx,
            &NewEntry {
                school_year_id: req.school_year_id,
                fiscal_month_id: req.fiscal_month_id,
                program_id: r.program_id,
                program_group_id: r.program_group_id,
                student_id,
                fund_type_id: r.fund_type_id,
                amount_item_id: item.id,
                flow: r.row.flow.clone(),
                entry_kind: r.row.entry_kind.clone(),
                amount: r.row.amount,
                stage: None,
                processed_on: None,
                counterparty: Some(r.row.counterparty.clone()),
                upload_file_id: Some(file_id),
                source_row_id: src,
                reason: None,
            },
        )?;
        entry_count += 1;
        match r.row.flow.as_str() {
            "collection" => expect_coll += r.row.amount,
            "expenditure" => expect_exp += r.row.amount,
            _ => {}
        }
    }

    // 검산 — 미리보기 합계 == 실제 원장 합계. 다르면 트랜잭션이 통째로 되돌아간다.
    let (coll, exp, n) = active_totals(tx, file_id)?;
    if coll != expect_coll || exp != expect_exp || n != entry_count
        || coll != built.preview.summary.collection_total || exp != built.preview.summary.expenditure_total
    {
        return Err(AppError::new("VERIFY", "반영한 금액이 미리보기와 달라 반영을 되돌렸습니다.").detail(format!(
            "coll {coll}/{expect_coll}/{} exp {exp}/{expect_exp}/{} n {n}/{entry_count}",
            built.preview.summary.collection_total, built.preview.summary.expenditure_total
        )));
    }
    record_apply(tx, file_id, "apply", n, coll, exp, "")?;
    audit::write(
        tx,
        "upload.apply",
        "upload_file",
        Some(file_id),
        &format!("'{}' 반영 (제{}판 · {} · 거래 {}건)", file_name, version_no, kind_label(&req.data_kind), n),
        json!({ "replaceFileId": req.replace_file_id, "entryCount": n }),
    )?;
    Ok(CommitResult { file: get(tx, file_id)?, entry_count: n, collection_total: coll, expenditure_total: exp })
}

fn active_totals(conn: &Connection, file_id: i64) -> AppResult<(i64, i64, i64)> {
    Ok(conn.query_row(
        "SELECT coalesce(sum(CASE WHEN flow = 'collection' THEN amount END), 0),
                coalesce(sum(CASE WHEN flow = 'expenditure' THEN amount END), 0),
                count(*)
           FROM ledger_entry WHERE upload_file_id = ?1 AND state = 'active'",
        [file_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?)
}

fn record_apply(conn: &Connection, file_id: i64, action: &str, n: i64, coll: i64, exp: i64, note: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO upload_apply (upload_file_id, action, entry_count, collection_total, expenditure_total, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![file_id, action, n, coll, exp, note],
    )?;
    Ok(())
}

/// 파일의 살아 있는 거래를 전부 취소한다. 돌려주는 값: (건수, 징수 합, 지출 합).
fn void_file_entries(conn: &Connection, file_id: i64, reason: &str) -> AppResult<(i64, i64, i64)> {
    let (coll, exp, n) = active_totals(conn, file_id)?;
    let ids: Vec<i64> = {
        let mut st = conn.prepare("SELECT id FROM ledger_entry WHERE upload_file_id = ?1 AND state = 'active' AND transfer_group_id IS NULL")?;
        let rows = st.query_map([file_id], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    crate::repo::allocation::void_for_entries(conn, &ids, reason)?;
    for id in ids {
        ledger::void(conn, id, "input_error", reason)?;
    }
    Ok((n, coll, exp))
}

// ── 4. 취소 · 복원 ──────────────────────────────────────────────────────

fn guard_month(conn: &Connection, file: &UploadFile) -> AppResult<()> {
    if let Some(m) = file.fiscal_month_id {
        if year::month_is_closed(conn, m)? {
            return Err(AppError::invalid(format!(
                "{} 은(는) 확정된 달입니다. 월별 정산에서 재개한 뒤 바꿀 수 있습니다.",
                file.fiscal_month_label
            )));
        }
    }
    Ok(())
}

pub fn cancel(tx: &Connection, file_id: i64, reason: &str) -> AppResult<UploadFile> {
    let f = get(tx, file_id)?;
    if f.status != "applied" {
        return Err(AppError::invalid("적용 중인 파일만 취소할 수 있습니다."));
    }
    guard_month(tx, &f)?;
    let r = reason.trim();
    if r.is_empty() {
        return Err(AppError::invalid("취소 사유를 입력해 주세요."));
    }
    let (n, coll, exp) = void_file_entries(tx, file_id, r)?;
    tx.execute(
        "UPDATE upload_file SET status = 'cancelled', cancelled_at = datetime('now', 'localtime') WHERE id = ?1",
        [file_id],
    )?;
    record_apply(tx, file_id, "cancel", n, coll, exp, r)?;
    audit::write(tx, "upload.cancel", "upload_file", Some(file_id), &format!("'{}' 취소 (거래 {}건): {r}", f.file_name, n), json!({}))?;
    Ok(get(tx, file_id)?)
}

/// 저장된 정규화 행으로 거래를 **새로** 만든다 (복원·재적용).
fn apply_from_rows(tx: &Connection, file: &UploadFile) -> AppResult<(i64, i64, i64)> {
    let month = file.fiscal_month_id.ok_or_else(|| AppError::invalid("귀속월이 없는 파일은 복원할 수 없습니다."))?;
    let items = catalog::amount_items(tx)?;
    struct R {
        source_row_id: Option<i64>,
        program_id: Option<i64>,
        program_group_id: Option<i64>,
        student_id: Option<i64>,
        fund_type_id: Option<i64>,
        item_code: String,
        entry_kind: String,
        flow: String,
        amount: i64,
        counterparty: String,
    }
    let rows: Vec<R> = {
        let mut st = tx.prepare(
            "SELECT source_row_id, program_id, program_group_id, student_id, fund_type_id, amount_item_code,
                    entry_kind, flow, amount, counterparty
               FROM upload_row WHERE upload_file_id = ?1 AND included = 1 AND status <> 'error' ORDER BY seq",
        )?;
        let it = st.query_map([file.id], |r| {
            Ok(R {
                source_row_id: r.get(0)?,
                program_id: r.get(1)?,
                program_group_id: r.get(2)?,
                student_id: r.get(3)?,
                fund_type_id: r.get(4)?,
                item_code: r.get(5)?,
                entry_kind: r.get(6)?,
                flow: r.get(7)?,
                amount: r.get(8)?,
                counterparty: r.get(9)?,
            })
        })?;
        it.collect::<Result<_, _>>()?
    };
    let mut expect = (0i64, 0i64, 0i64);
    for r in &rows {
        let item = items
            .iter()
            .find(|i| i.code == r.item_code)
            .ok_or_else(|| AppError::invalid(format!("금액 항목 '{}' 이(가) 없습니다.", r.item_code)))?;
        ledger::insert(
            tx,
            &NewEntry {
                school_year_id: file.school_year_id,
                fiscal_month_id: month,
                program_id: r.program_id,
                program_group_id: r.program_group_id,
                student_id: r.student_id,
                fund_type_id: r.fund_type_id,
                amount_item_id: item.id,
                flow: r.flow.clone(),
                entry_kind: r.entry_kind.clone(),
                amount: r.amount,
                stage: None,
                processed_on: None,
                counterparty: Some(r.counterparty.clone()),
                upload_file_id: Some(file.id),
                source_row_id: r.source_row_id,
                reason: Some("이전 판 복원".into()),
            },
        )?;
        expect.0 += 1;
        match r.flow.as_str() {
            "collection" => expect.1 += r.amount,
            "expenditure" => expect.2 += r.amount,
            _ => {}
        }
    }
    let (coll, exp, n) = active_totals(tx, file.id)?;
    if (n, coll, exp) != expect {
        return Err(AppError::new("VERIFY", "복원한 금액이 저장된 행과 달라 복원을 되돌렸습니다.")
            .detail(format!("{:?} vs {:?}", (n, coll, exp), expect)));
    }
    Ok((n, coll, exp))
}

/// 이전 판(또는 취소된 판)을 다시 적용한다. 현재 적용 중인 판은 취소된다.
pub fn restore(tx: &Connection, file_id: i64, reason: &str) -> AppResult<UploadFile> {
    let target = get(tx, file_id)?;
    if target.status != "replaced" && target.status != "cancelled" {
        return Err(AppError::invalid("이전 버전이나 취소된 파일만 복원할 수 있습니다."));
    }
    guard_month(tx, &target)?;
    let r = reason.trim();
    if r.is_empty() {
        return Err(AppError::invalid("복원 사유를 입력해 주세요."));
    }
    // 같은 계보(또는 같은 달·유형)에서 적용 중인 판을 먼저 취소한다.
    let mut current: Option<UploadFile> = lineage(tx, file_id)?.into_iter().find(|f| f.status == "applied");
    if current.is_none() {
        if let (Some(m), true) = (target.fiscal_month_id, exclusive_kind(&target.data_kind)) {
            current = find_applied(tx, m, &target.data_kind)?;
        }
    }
    if let Some(cur) = &current {
        let (n, coll, exp) = void_file_entries(tx, cur.id, &format!("제{}판 복원으로 취소", target.version_no))?;
        tx.execute(
            "UPDATE upload_file SET status = 'replaced', cancelled_at = datetime('now', 'localtime') WHERE id = ?1",
            [cur.id],
        )?;
        record_apply(tx, cur.id, "replace", n, coll, exp, &format!("제{}판 복원으로 취소", target.version_no))?;
    }
    let (n, coll, exp) = apply_from_rows(tx, &target)?;
    tx.execute(
        "UPDATE upload_file SET status = 'applied', applied_at = datetime('now', 'localtime'), cancelled_at = NULL, replaced_by_id = NULL WHERE id = ?1",
        [file_id],
    )?;
    record_apply(tx, file_id, "restore", n, coll, exp, r)?;
    audit::write(
        tx,
        "upload.restore",
        "upload_file",
        Some(file_id),
        &format!("'{}' 제{}판 복원 (거래 {}건): {r}", target.file_name, target.version_no, n),
        json!({ "cancelledFileId": current.map(|c| c.id) }),
    )?;
    Ok(get(tx, file_id)?)
}

// ── 5. 영향 미리보기 (취소·복원 전) ─────────────────────────────────────

fn totals_of_entries(conn: &Connection, file_id: i64) -> AppResult<Totals> {
    let (coll, exp, n) = active_totals(conn, file_id)?;
    let programs: i64 = conn.query_row(
        "SELECT count(DISTINCT coalesce(program_id, -program_group_id)) FROM ledger_entry WHERE upload_file_id = ?1 AND state = 'active' AND (program_id IS NOT NULL OR program_group_id IS NOT NULL)",
        [file_id],
        |r| r.get(0),
    )?;
    let mut st = conn.prepare(
        "SELECT coalesce(f.name, '미확정'), count(*), sum(e.amount)
           FROM ledger_entry e LEFT JOIN fund_type f0 ON f0.id = e.fund_type_id
           LEFT JOIN fund_type f ON f.id = coalesce(f0.merged_into_id, f0.id)
          WHERE e.upload_file_id = ?1 AND e.state = 'active'
          GROUP BY f.id ORDER BY f.sort_order",
    )?;
    let by_fund = st
        .query_map([file_id], |r| Ok(SummaryLine { name: r.get(0)?, count: r.get(1)?, total: r.get(2)? }))?
        .collect::<Result<_, _>>()?;
    Ok(Totals { entry_count: n, collection_total: coll, expenditure_total: exp, program_count: programs, by_fund })
}

fn totals_of_rows(conn: &Connection, file_id: i64) -> AppResult<Totals> {
    let (coll, exp, n, programs): (i64, i64, i64, i64) = conn.query_row(
        "SELECT coalesce(sum(CASE WHEN flow = 'collection' THEN amount END), 0),
                coalesce(sum(CASE WHEN flow = 'expenditure' THEN amount END), 0),
                count(*),
                count(DISTINCT coalesce(program_id, -program_group_id))
           FROM upload_row WHERE upload_file_id = ?1 AND included = 1 AND status <> 'error'",
        [file_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let mut st = conn.prepare(
        "SELECT coalesce(f.name, '미확정'), count(*), sum(u.amount)
           FROM upload_row u LEFT JOIN fund_type f0 ON f0.id = u.fund_type_id
           LEFT JOIN fund_type f ON f.id = coalesce(f0.merged_into_id, f0.id)
          WHERE u.upload_file_id = ?1 AND u.included = 1 AND u.status <> 'error'
          GROUP BY f.id ORDER BY f.sort_order",
    )?;
    let by_fund = st
        .query_map([file_id], |r| Ok(SummaryLine { name: r.get(0)?, count: r.get(1)?, total: r.get(2)? }))?
        .collect::<Result<_, _>>()?;
    Ok(Totals { entry_count: n, collection_total: coll, expenditure_total: exp, program_count: programs, by_fund })
}

/// 이 파일의 거래와 학생·프로그램이 겹치는 **다른 파일**의 살아 있는 거래 — 연결 재검증 대상.
fn dependents(conn: &Connection, file: &UploadFile) -> AppResult<Vec<Dependent>> {
    let Some(month) = file.fiscal_month_id else { return Ok(Vec::new()) };
    let mut st = conn.prepare(
        "SELECT o.id, o.file_name, o.data_kind, count(e2.id)
           FROM ledger_entry e2
           JOIN upload_file o ON o.id = e2.upload_file_id
          WHERE e2.state = 'active' AND e2.upload_file_id <> ?1 AND e2.fiscal_month_id = ?2
            AND EXISTS (SELECT 1 FROM ledger_entry e1
                         WHERE e1.upload_file_id = ?1 AND e1.state = 'active'
                           AND ((e1.student_id IS NOT NULL AND e1.student_id = e2.student_id)
                             OR (e1.program_id IS NOT NULL AND e1.program_id = e2.program_id)))
          GROUP BY o.id ORDER BY o.id",
    )?;
    let rows = st.query_map(params![file.id, month], |r| {
        let kind: String = r.get(2)?;
        Ok(Dependent { file_id: r.get(0)?, file_name: r.get(1)?, data_kind_label: kind_label(&kind), entry_count: r.get(3)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn impact(conn: &Connection, file_id: i64, action: &str) -> AppResult<Impact> {
    let file = get(conn, file_id)?;
    let mut blocked = Vec::new();
    let mut warnings = Vec::new();
    if let Some(m) = file.fiscal_month_id {
        if year::month_is_closed(conn, m)? {
            blocked.push(format!("{} 은(는) 확정된 달입니다. 재개한 뒤 바꿀 수 있습니다.", file.fiscal_month_label));
        }
    }
    let (current, before, after) = match action {
        "cancel" => {
            if file.status != "applied" {
                blocked.push("적용 중인 파일만 취소할 수 있습니다.".into());
            }
            let before = totals_of_entries(conn, file_id)?;
            (None, before, Totals::default())
        }
        "restore" => {
            if file.status != "replaced" && file.status != "cancelled" {
                blocked.push("이전 버전이나 취소된 파일만 복원할 수 있습니다.".into());
            }
            let mut current = lineage(conn, file_id)?.into_iter().find(|f| f.status == "applied");
            if current.is_none() {
                if let (Some(m), true) = (file.fiscal_month_id, exclusive_kind(&file.data_kind)) {
                    current = find_applied(conn, m, &file.data_kind)?;
                }
            }
            let before = match &current {
                Some(c) => totals_of_entries(conn, c.id)?,
                None => Totals::default(),
            };
            let after = totals_of_rows(conn, file_id)?;
            if after.entry_count == 0 && is_ledger_kind(&file.data_kind) {
                blocked.push("복원할 정규화 행이 없습니다.".into());
            }
            (current, before, after)
        }
        _ => return Err(AppError::invalid("알 수 없는 작업입니다.")),
    };
    let dep_source = current.as_ref().unwrap_or(&file);
    let dependents = dependents(conn, dep_source)?;
    if !dependents.is_empty() {
        warnings.push(format!(
            "같은 달의 다른 파일 {}개에 같은 학생·프로그램의 거래가 있습니다. 그 거래는 유지되지만 연결 상태를 다시 확인해 주세요.",
            dependents.len()
        ));
    }
    if before.collection_total == after.collection_total && before.expenditure_total == after.expenditure_total && before != after {
        warnings.push("금액 합계는 같지만 거래 건수·프로그램·재원 구성이 달라집니다.".into());
    }
    Ok(Impact {
        action: action.into(),
        month_label: file.fiscal_month_label.clone(),
        file,
        current,
        before,
        after,
        dependents,
        blocked,
        warnings,
    })
}

// ── 6. 상세 ──────────────────────────────────────────────────────────────

pub fn applies(conn: &Connection, file_id: i64) -> AppResult<Vec<UploadApply>> {
    let mut st = conn.prepare(
        "SELECT id, action, entry_count, collection_total, expenditure_total, note, created_at
           FROM upload_apply WHERE upload_file_id = ?1 ORDER BY id",
    )?;
    let rows = st.query_map([file_id], |r| {
        Ok(UploadApply {
            id: r.get(0)?,
            action: r.get(1)?,
            entry_count: r.get(2)?,
            collection_total: r.get(3)?,
            expenditure_total: r.get(4)?,
            note: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn detail(conn: &Connection, file_id: i64) -> AppResult<UploadDetail> {
    let file = get(conn, file_id)?;
    let items = catalog::amount_items(conn)?;
    let funds = catalog::fund_types(conn)?;
    let mut st = conn.prepare(
        "SELECT u.seq, u.sheet_name, u.row_no, u.program_text, u.program_id, u.program_group_id, u.grade, u.class_name,
                u.student_no, u.student_name, u.fund_text, u.fund_type_id, u.amount_item_code, u.entry_kind, u.amount,
                u.counterparty, u.status, u.included, u.issues_json, p.name, g.name
           FROM upload_row u LEFT JOIN program p ON p.id = u.program_id LEFT JOIN program_group g ON g.id = u.program_group_id
          WHERE u.upload_file_id = ?1 ORDER BY u.seq",
    )?;
    let rows = st.query_map([file_id], |r| {
        let fund_id: Option<i64> = r.get(11)?;
        let item_code: String = r.get(12)?;
        let pname: Option<String> = r.get(19)?;
        let gname: Option<String> = r.get(20)?;
        let student = StudentKey { grade: r.get(6)?, class_name: r.get(7)?, student_no: r.get(8)?, name: r.get(9)? };
        Ok(PreviewRow {
            seq: r.get(0)?,
            sheet: r.get(1)?,
            row_no: r.get(2)?,
            program_text: r.get(3)?,
            program_name: pname.or_else(|| gname.map(|g| format!("{g} (그룹)"))).unwrap_or_default(),
            program_suggestion: String::new(),
            student: student_label(&Some(student)),
            fund_text: r.get(10)?,
            fund_name: fund_id.and_then(|i| funds.iter().find(|f| f.id == i)).map(|f| f.name.clone()).unwrap_or_else(|| "미확정".into()),
            item_name: items.iter().find(|i| i.code == item_code).map(|i| i.name.clone()).unwrap_or_default(),
            entry_kind: r.get(13)?,
            amount: r.get(14)?,
            counterparty: r.get(15)?,
            status: r.get(16)?,
            included: r.get::<_, i64>(17)? == 1,
            issues: serde_json::from_str(&r.get::<_, String>(18)?).unwrap_or_default(),
        })
    })?;
    let rows: Vec<PreviewRow> = rows.collect::<Result<_, _>>()?;
    let mut st = conn.prepare("SELECT sheet_name, row_no, cells_json FROM source_row WHERE upload_file_id = ?1 ORDER BY id")?;
    let src = st.query_map([file_id], |r| {
        let cells: Vec<String> = serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default();
        Ok(SourceRow { sheet: r.get(0)?, row_no: r.get(1)?, cells })
    })?;
    Ok(UploadDetail {
        versions: lineage(conn, file_id)?,
        applies: applies(conn, file_id)?,
        source_rows: src.collect::<Result<_, _>>()?,
        rows,
        file,
    })
}

/// 금액 표기(시험·로그용).
pub fn won(v: i64) -> String {
    money::format_won(v)
}

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;
