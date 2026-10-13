//! 세부반 → 통합 프로그램 연결 (P7-1.1).
//!
//! 학생 징수자료는 세부반('로봇과학A'·'로봇과학B') 단위, 지출자료는 프로그램('로봇과학') 단위다. 둘 다 `program` 행으로
//! 그대로 두고(원본 표기·원장 불변) 세부반이 어느 통합 프로그램에 속하는지만 `program_link` 가 잇는다.
//!   · 추천은 저장하지 않고 매번 계산한다(`suggestions`). 표에 있는 연결은 전부 사용자가 확정한 것이다.
//!   · 추천 조건: 이름 규칙(끝의 반 구분)에 맞고 **그 바탕 이름의 프로그램에 지급 자료가 있거나 이미 통합 프로그램**일 때.
//!     'ABC' → 'AB' 같은 영문 끝 글자, 그룹과 이름이 같은 경우, 통합 프로그램이 다시 세부반이 되는 경우는 추천하지 않는다.
//!   · 세부반 하나는 통합 프로그램 하나에만, 깊이는 한 단계, 그룹 구성원은 세부반이 될 수 없다.
//!   · 세부반에 확정된 달의 거래가 있으면 연결을 바꾸지 못한다(월 확정 보호 — 재개 뒤 변경).
//!   · 모든 변경은 `program_link_log`(지우지 않음)와 audit_log 에 남는다. 대조 결과는 저장하지 않으므로 연결이 바뀌면 다음 조회부터 다시 계산된다.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::money::format_won as w;
use crate::domain::program_link::split_class_suffix;
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::repo::{audit, program, year};

/// 확정된 연결: 세부반 id → 통합 프로그램 id
pub fn links(conn: &Connection, school_year_id: i64) -> AppResult<HashMap<i64, i64>> {
    let mut st = conn.prepare("SELECT program_id, parent_id FROM program_link WHERE school_year_id = ?1")?;
    let rows = st.query_map([school_year_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn parent_of(conn: &Connection, program_id: i64) -> AppResult<Option<i64>> {
    Ok(conn.query_row("SELECT parent_id FROM program_link WHERE program_id = ?1", [program_id], |r| r.get(0)).optional()?)
}

pub fn children_of(conn: &Connection, parent_id: i64) -> AppResult<Vec<i64>> {
    let mut st = conn.prepare("SELECT l.program_id FROM program_link l JOIN program p ON p.id = l.program_id WHERE l.parent_id = ?1 ORDER BY p.name")?;
    let rows = st.query_map([parent_id], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn action_label(a: &str) -> &'static str {
    match a {
        "confirm" => "연결 확정",
        "change" => "연결 변경",
        "unlink" => "연결 해제",
        "noop" => "변경 없음",
        _ => "알 수 없음",
    }
}

pub fn status_label(s: &str) -> &'static str {
    match s {
        "confirmed" => "확정",
        "suggested" => "추천",
        "candidate" => "후보 (조건 미충족)",
        "none" => "—",
        _ => "알 수 없음",
    }
}

// ── 프로그램 현황 적재 ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Info {
    id: i64,
    name: String,
    parent: Option<i64>,
    linked_at: Option<String>,
    rule: String,
    child_ids: Vec<i64>,
    group: Option<String>,
    coll_count: i64,
    coll_total: i64,
    exp_count: i64,
    exp_total: i64,
    aliases: Vec<String>,
}

fn load(conn: &Connection, school_year_id: i64) -> AppResult<Vec<Info>> {
    load_with(conn, school_year_id, None)
}

/// `links_override` 가 있으면 저장된 연결 대신 그것을 적용한 상태로 읽는다 (미리보기용).
fn load_with(conn: &Connection, school_year_id: i64, links_override: Option<&HashMap<i64, i64>>) -> AppResult<Vec<Info>> {
    let mut st = conn.prepare("SELECT id, name FROM program WHERE school_year_id = ?1 ORDER BY name")?;
    let mut infos: Vec<Info> = st
        .query_map([school_year_id], |r| {
            Ok(Info {
                id: r.get(0)?,
                name: r.get(1)?,
                parent: None,
                linked_at: None,
                rule: String::new(),
                child_ids: vec![],
                group: None,
                coll_count: 0,
                coll_total: 0,
                exp_count: 0,
                exp_total: 0,
                aliases: vec![],
            })
        })?
        .collect::<Result<_, _>>()?;
    let idx: HashMap<i64, usize> = infos.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    {
        let mut st = conn.prepare("SELECT program_id, parent_id, updated_at, rule FROM program_link WHERE school_year_id = ?1")?;
        let mut rows: Vec<(i64, i64, String, String)> = st.query_map([school_year_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        if let Some(o) = links_override {
            rows = o.iter().map(|(c, p)| (*c, *p, String::new(), String::from("preview"))).collect();
        }
        for (c, p, at, rule) in rows {
            if let Some(&i) = idx.get(&c) {
                infos[i].parent = Some(p);
                infos[i].linked_at = Some(at);
                infos[i].rule = rule;
            }
            if let Some(&i) = idx.get(&p) {
                infos[i].child_ids.push(c);
            }
        }
    }
    {
        let mut st = conn.prepare("SELECT m.program_id, g.name FROM program_group_member m JOIN program_group g ON g.id = m.group_id WHERE g.school_year_id = ?1")?;
        let rows: Vec<(i64, String)> = st.query_map([school_year_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        for (p, g) in rows {
            if let Some(&i) = idx.get(&p) {
                infos[i].group = Some(g);
            }
        }
    }
    {
        let mut st = conn.prepare(
            "SELECT program_id, flow, count(*), coalesce(sum(amount), 0) FROM ledger_entry
              WHERE school_year_id = ?1 AND state = 'active' AND stage = 'actual' AND program_id IS NOT NULL AND flow IN ('collection', 'expenditure')
              GROUP BY program_id, flow",
        )?;
        let rows: Vec<(i64, String, i64, i64)> = st.query_map([school_year_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        for (p, flow, n, sum) in rows {
            if let Some(&i) = idx.get(&p) {
                if flow == "collection" {
                    infos[i].coll_count += n;
                    infos[i].coll_total += sum;
                } else {
                    infos[i].exp_count += n;
                    infos[i].exp_total += sum;
                }
            }
        }
        let mut st = conn.prepare(
            "SELECT a.program_id, count(*), coalesce(sum(a.amount), 0) FROM allocation a JOIN ledger_entry e ON e.id = a.entry_id
              WHERE e.school_year_id = ?1 AND a.status = 'active' AND e.state = 'active' AND e.stage = 'actual' GROUP BY a.program_id",
        )?;
        let rows: Vec<(i64, i64, i64)> = st.query_map([school_year_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        for (p, n, sum) in rows {
            if let Some(&i) = idx.get(&p) {
                infos[i].coll_count += n;
                infos[i].coll_total += sum;
            }
        }
    }
    {
        let mut st = conn.prepare("SELECT program_id, alias FROM program_alias WHERE school_year_id = ?1 AND program_id IS NOT NULL ORDER BY alias")?;
        let rows: Vec<(i64, String)> = st.query_map([school_year_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        for (p, a) in rows {
            if let Some(&i) = idx.get(&p) {
                infos[i].aliases.push(a);
            }
        }
    }
    Ok(infos)
}

// ── 자동 추천 ───────────────────────────────────────────────────────────

fn suggest_for(conn: &Connection, school_year_id: i64, p: &Info, by_id: &HashMap<i64, &Info>) -> AppResult<Option<LinkSuggestion>> {
    if p.parent.is_some() || !p.child_ids.is_empty() || p.group.is_some() {
        return Ok(None);
    }
    let Some((base, _suffix)) = split_class_suffix(&p.name) else {
        return Ok(None);
    };
    let target = program::resolve_target(conn, school_year_id, &base)?;
    let (parent_id, parent_name, recommended, reason) = match target {
        None => return Ok(None),
        Some(program::Target::Group(g)) => (0, program::group_name_of(conn, g)?, false, format!("'{base}' 은(는) 프로그램 그룹이라 세부반 연결 대상이 아닙니다. 그룹 지급은 그룹 단위로 검증됩니다.")),
        Some(program::Target::Program(q)) => {
            if q == p.id {
                return Ok(None);
            }
            let Some(info) = by_id.get(&q) else { return Ok(None) };
            if info.parent.is_some() {
                (q, info.name.clone(), false, format!("'{}' 이(가) 이미 다른 통합 프로그램의 세부반이라 통합 프로그램이 될 수 없습니다.", info.name))
            } else if let Some(g) = &info.group {
                (q, info.name.clone(), true, format!("'{}' 은(는) 프로그램 그룹 '{}' 의 구성원으로 그룹 지급에 포함됩니다.", info.name, g))
            } else if !info.child_ids.is_empty() {
                (q, info.name.clone(), true, format!("'{}' 은(는) 이미 세부반 {}개를 가진 통합 프로그램입니다.", info.name, info.child_ids.len()))
            } else if info.exp_count > 0 {
                (q, info.name.clone(), true, format!("지급 자료 {}건({})이 있는 '{}' 와(과) 반 구분만 다릅니다.", info.exp_count, w(info.exp_total), info.name))
            } else {
                (q, info.name.clone(), false, format!("'{}' 에 아직 지급 자료가 없어 추천하지 않습니다. 지급 자료를 올리면 추천되며, 지금 직접 연결할 수도 있습니다.", info.name))
            }
        }
    };
    if parent_id == 0 {
        return Ok(Some(LinkSuggestion { program_id: p.id, program_name: p.name.clone(), parent_id: 0, parent_name, recommended: false, reason }));
    }
    Ok(Some(LinkSuggestion { program_id: p.id, program_name: p.name.clone(), parent_id, parent_name, recommended, reason }))
}

/// 모든 프로그램의 추천·후보 (확정된 것·그룹 구성원·통합 프로그램 자신은 제외).
pub fn suggestions(conn: &Connection, school_year_id: i64) -> AppResult<Vec<LinkSuggestion>> {
    suggestions_with(conn, school_year_id, None)
}

pub fn suggestions_with(conn: &Connection, school_year_id: i64, links_override: Option<&HashMap<i64, i64>>) -> AppResult<Vec<LinkSuggestion>> {
    let infos = load_with(conn, school_year_id, links_override)?;
    let by_id: HashMap<i64, &Info> = infos.iter().map(|p| (p.id, p)).collect();
    let mut out = Vec::new();
    for p in &infos {
        if let Some(s) = suggest_for(conn, school_year_id, p, &by_id)? {
            out.push(s);
        }
    }
    Ok(out)
}

/// 아직 확정되지 않은 추천 (recommended 만) — 대조 화면의 '매핑 확인 필요' 판정에 쓴다.
pub fn pending(conn: &Connection, school_year_id: i64) -> AppResult<Vec<LinkSuggestion>> {
    pending_with(conn, school_year_id, None)
}

pub fn pending_with(conn: &Connection, school_year_id: i64, links_override: Option<&HashMap<i64, i64>>) -> AppResult<Vec<LinkSuggestion>> {
    Ok(suggestions_with(conn, school_year_id, links_override)?.into_iter().filter(|s| s.recommended).collect())
}

fn ym_label(ym: &str) -> String {
    let mut it = ym.split('-');
    match (it.next(), it.next()) {
        (Some(y), Some(m)) => format!("{y}년 {}월", m.trim_start_matches('0')),
        _ => ym.to_string(),
    }
}

/// 영향 월 산출 — 연결 변경 전후로 **실제 검증 결과(프로그램×항목 행)가 달라지는 달**. 이름이 아니라 프로그램 id·연결 관계로,
/// 세부반·기존/새 통합 프로그램·그룹·배분·재원경정·환불·추천 판정까지 대조 엔진이 그대로 계산한 결과를 비교하므로 어떤 경로의 간접 영향도 빠지지 않는다.
/// (달라지는 행 수, 영향 달) — 학년도의 거래가 있는 달만 본다 (다른 학년도는 무관).
fn affected_months(conn: &Connection, school_year_id: i64, after: &HashMap<i64, i64>) -> AppResult<Vec<(i64, String, bool, i64)>> {
    let mut st = conn.prepare("SELECT m.id, m.ym, m.status FROM fiscal_month m WHERE m.school_year_id = ?1 AND EXISTS (SELECT 1 FROM ledger_entry e WHERE e.fiscal_month_id = m.id) ORDER BY m.seq")?;
    let months: Vec<(i64, String, String)> = st.query_map([school_year_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, ym, status) in months {
        let before = crate::repo::recon::row_digests(conn, id, None)?;
        let now = crate::repo::recon::row_digests(conn, id, Some(after))?;
        let mut changed = 0i64;
        for (k, v) in &now {
            if before.get(k) != Some(v) {
                changed += 1;
            }
        }
        changed += before.keys().filter(|k| !now.contains_key(*k)).count() as i64;
        if changed > 0 {
            out.push((id, ym_label(&ym), status == "closed", changed));
        }
    }
    Ok(out)
}

// ── 화면 ─────────────────────────────────────────────────────────────────

fn to_row(p: &Info, by_id: &HashMap<i64, &Info>, s: Option<LinkSuggestion>) -> LinkRow {
    let role = if !p.child_ids.is_empty() { "parent" } else if p.parent.is_some() { "child" } else { "none" };
    let status = if p.parent.is_some() {
        "confirmed"
    } else {
        match &s {
            Some(x) if x.recommended => "suggested",
            Some(_) => "candidate",
            None => "none",
        }
    };
    LinkRow {
        program_id: p.id,
        name: p.name.clone(),
        role: role.into(),
        parent_id: p.parent,
        parent_name: p.parent.and_then(|q| by_id.get(&q).map(|x| x.name.clone())),
        linked_at: p.linked_at.clone(),
        rule: p.rule.clone(),
        group_name: p.group.clone(),
        child_names: p.child_ids.iter().filter_map(|c| by_id.get(c).map(|x| x.name.clone())).collect(),
        suggestion: s,
        status: status.into(),
        status_label: status_label(status).into(),
        collection_count: p.coll_count,
        collection_total: p.coll_total,
        expense_count: p.exp_count,
        expense_total: p.exp_total,
        aliases: p.aliases.clone(),
    }
}

pub fn board(conn: &Connection, school_year_id: i64) -> AppResult<LinkBoard> {
    let infos = load(conn, school_year_id)?;
    let by_id: HashMap<i64, &Info> = infos.iter().map(|p| (p.id, p)).collect();
    let mut rows = Vec::new();
    for p in &infos {
        let s = suggest_for(conn, school_year_id, p, &by_id)?;
        rows.push(to_row(p, &by_id, s));
    }
    let mut parents = Vec::new();
    for p in infos.iter().filter(|p| !p.child_ids.is_empty()) {
        let children: Vec<LinkRow> = rows.iter().filter(|r| r.parent_id == Some(p.id)).cloned().collect();
        let collection_total = p.coll_total + children.iter().map(|c| c.collection_total).sum::<i64>();
        let expense_total = p.exp_total + children.iter().map(|c| c.expense_total).sum::<i64>();
        parents.push(LinkParentView { parent_id: p.id, name: p.name.clone(), children, expense_total, collection_total });
    }
    Ok(LinkBoard {
        school_year_id,
        pending_count: rows.iter().filter(|r| r.status == "suggested").count() as i64,
        confirmed_count: rows.iter().filter(|r| r.status == "confirmed").count() as i64,
        candidate_count: rows.iter().filter(|r| r.status == "candidate").count() as i64,
        rows,
        parents,
    })
}

// ── 미리보기 · 적용 ─────────────────────────────────────────────────────

/// 세부반에 거래(직접 귀속·배분)가 있는 달 — 연결을 바꾸면 이 달들의 대조 결과가 달라진다.
fn months_of(conn: &Connection, program_id: i64) -> AppResult<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT DISTINCT fiscal_month_id FROM ledger_entry WHERE program_id = ?1 AND state = 'active'
         UNION SELECT DISTINCT e.fiscal_month_id FROM allocation a JOIN ledger_entry e ON e.id = a.entry_id WHERE a.program_id = ?1 AND a.status = 'active' AND e.state = 'active'",
    )?;
    let rows = st.query_map([program_id], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn preview(conn: &Connection, school_year_id: i64, items: &[LinkItem]) -> AppResult<LinkPreview> {
    let infos = load(conn, school_year_id)?;
    let by_id: HashMap<i64, &Info> = infos.iter().map(|p| (p.id, p)).collect();
    let mut out = Vec::new();
    let mut seen: BTreeSet<i64> = BTreeSet::new();
    let batch_children: BTreeSet<i64> = items.iter().filter(|i| i.parent_id.is_some()).map(|i| i.program_id).collect();
    let batch_parents: BTreeSet<i64> = items.iter().filter_map(|i| i.parent_id).collect();
    let mut month_map: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    let mut warnings_all: Vec<String> = Vec::new();
    for it in items {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let Some(p) = by_id.get(&it.program_id) else {
            out.push(LinkPreviewItem { program_id: it.program_id, program_name: format!("프로그램 #{}", it.program_id), from_parent_id: None, from_parent_name: None, to_parent_id: it.parent_id, to_parent_name: None, action: "noop".into(), action_label: action_label("noop").into(), collection_total: 0, expense_total: 0, warnings, errors: vec!["이 학년도의 프로그램이 아닙니다.".into()] });
            continue;
        };
        if !seen.insert(p.id) {
            errors.push("같은 세부반이 한 번에 두 번 들어 있습니다.".into());
        }
        if it.expected_known && it.expected_parent_id != p.parent {
            errors.push(format!(
                "미리보기 이후 연결이 바뀌었습니다 (지금: {}). 화면을 새로 고친 뒤 다시 확인해 주세요.",
                p.parent.and_then(|q| by_id.get(&q)).map(|x| x.name.as_str()).unwrap_or("연결 없음")
            ));
        }
        let to = it.parent_id;
        let to_info = to.and_then(|q| by_id.get(&q).copied());
        let action = match (p.parent, to) {
            (None, None) => "noop",
            (None, Some(_)) => "confirm",
            (Some(a), Some(b)) if a == b => "noop",
            (Some(_), Some(_)) => "change",
            (Some(_), None) => "unlink",
        };
        if let Some(q) = to {
            match to_info {
                None => errors.push("연결할 통합 프로그램이 이 학년도에 없습니다.".into()),
                Some(t) => {
                    if q == p.id {
                        errors.push("자기 자신에게 연결할 수 없습니다.".into());
                    }
                    if t.parent.is_some() || batch_children.contains(&q) {
                        errors.push(format!("'{}' 은(는) 다른 통합 프로그램의 세부반이라 통합 프로그램이 될 수 없습니다 (깊이는 한 단계).", t.name));
                    }
                    if !p.child_ids.is_empty() || batch_parents.contains(&p.id) {
                        errors.push(format!("'{}' 은(는) 이미 세부반을 가진 통합 프로그램이라 세부반이 될 수 없습니다.", p.name));
                    }
                    if let Some(g) = &p.group {
                        errors.push(format!("'{}' 은(는) 프로그램 그룹 '{}' 의 구성원이라 세부반이 될 수 없습니다 (그룹 지급 중복 방지).", p.name, g));
                    }
                    if p.exp_count > 0 {
                        warnings.push(format!("세부반 '{}' 에 직접 연결된 지급 {}건({})이 있습니다. 통합 지급액에 함께 합산되며, 지급 자료의 프로그램 표기를 확인해 주세요.", p.name, p.exp_count, w(p.exp_total)));
                    }
                    if t.coll_count > 0 && action != "noop" {
                        warnings.push(format!("통합 프로그램 '{}' 에도 직접 귀속된 징수 {}건({})이 있어 세부반 징수와 함께 합산됩니다.", t.name, t.coll_count, w(t.coll_total)));
                    }
                    if t.exp_count == 0 && t.child_ids.is_empty() && t.group.is_none() && action != "noop" {
                        warnings.push(format!("'{}' 에 아직 지급 자료가 없습니다. 연결은 되지만 지급 자료를 올리기 전에는 '자료 확인 필요' 로 남습니다.", t.name));
                    }
                }
            }
        }
        if action == "change" {
            warnings.push(format!("기존 연결('{}')을 바꿉니다. 바뀐 달의 대조 결과가 다시 계산됩니다.", p.parent.and_then(|q| by_id.get(&q)).map(|x| x.name.as_str()).unwrap_or("?")));
        }
        if action != "noop" && errors.is_empty() {
            for m in months_of(conn, p.id)? {
                month_map.entry(m).or_default().push(p.name.clone());
            }
        }
        out.push(LinkPreviewItem {
            program_id: p.id,
            program_name: p.name.clone(),
            from_parent_id: p.parent,
            from_parent_name: p.parent.and_then(|q| by_id.get(&q)).map(|x| x.name.clone()),
            to_parent_id: to,
            to_parent_name: to_info.map(|x| x.name.clone()),
            action: action.into(),
            action_label: action_label(action).into(),
            collection_total: p.coll_total,
            expense_total: p.exp_total,
            warnings,
            errors,
        });
    }
    let mut months = Vec::new();
    let mut block_reasons: Vec<String> = out.iter().flat_map(|i| i.errors.iter().map(|e| format!("{}: {e}", i.program_name))).collect();
    // 연결 변경을 적용한 상태를 만들어 달마다 실제 검증 결과를 다시 계산해 비교한다 (세부반 거래가 있는 달만 보는 것이 아니다).
    let mut after: HashMap<i64, i64> = infos.iter().filter_map(|p| p.parent.map(|q| (p.id, q))).collect();
    for i in out.iter().filter(|i| i.action != "noop" && i.errors.is_empty()) {
        match i.to_parent_id {
            Some(q) => {
                after.insert(i.program_id, q);
            }
            None => {
                after.remove(&i.program_id);
            }
        }
    }
    if block_reasons.is_empty() && out.iter().any(|i| i.action != "noop") {
        for (m, label, closed, changed) in affected_months(conn, school_year_id, &after)? {
            let mut names = month_map.get(&m).cloned().unwrap_or_default();
            if names.is_empty() {
                names.push("(판정 상태 변경)".into());
            }
            if closed {
                for i in out.iter().filter(|i| i.action != "noop" && i.errors.is_empty()) {
                    block_reasons.push(format!(
                        "'{}' ({} → {}): {label} 회계가 확정되어 있어 프로그램 연결을 변경할 수 없습니다. 해당 월을 재개한 후 매핑을 변경하고 다시 확정해 주세요. (달라지는 검증 행 {changed}개)",
                        i.program_name,
                        i.from_parent_name.as_deref().unwrap_or("연결 없음"),
                        i.to_parent_name.as_deref().unwrap_or("연결 해제")
                    ));
                }
            }
            months.push(LinkMonthImpact { fiscal_month_id: m, label, closed, program_names: names, changed_rows: changed });
        }
    }
    if !months.is_empty() {
        warnings_all.push(format!("{}개 달의 프로그램별 검증 결과가 다시 계산됩니다 (원장 거래는 바뀌지 않습니다).", months.len()));
    }
    let change_count = out.iter().filter(|i| i.action != "noop" && i.errors.is_empty()).count() as i64;
    Ok(LinkPreview { blocked: !block_reasons.is_empty(), block_reasons, warnings: warnings_all, change_count, items: out, months })
}

/// 적용 — 미리보기와 **같은 함수**로 다시 검증해 하나라도 막히면 아무것도 바꾸지 않는다(호출자가 트랜잭션 안에서 돌리므로 부분 적용 없음,
/// 차단된 변경은 감사 이력에 남지 않는다). rule: suggested(추천 확정) · manual(직접 선택).
pub fn apply(conn: &Connection, school_year_id: i64, items: &[LinkItem], note: &str, rule: &str) -> AppResult<LinkApplyResult> {
    let pv = preview(conn, school_year_id, items)?;
    if pv.blocked {
        return Err(AppError::invalid(pv.block_reasons.join(" ")));
    }
    let rule = if rule == "suggested" { "suggested" } else { "manual" };
    let mut log_ids = Vec::new();
    let mut applied = 0;
    for it in pv.items.iter().filter(|i| i.action != "noop") {
        match it.to_parent_id {
            Some(q) => {
                conn.execute(
                    "INSERT INTO program_link (school_year_id, program_id, parent_id, rule, note) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(program_id) DO UPDATE SET parent_id = excluded.parent_id, rule = excluded.rule, note = excluded.note, updated_at = datetime('now', 'localtime')",
                    params![school_year_id, it.program_id, q, rule, note.trim()],
                )?;
            }
            None => {
                conn.execute("DELETE FROM program_link WHERE program_id = ?1", [it.program_id])?;
            }
        }
        conn.execute(
            "INSERT INTO program_link_log (school_year_id, program_id, from_parent_id, to_parent_id, action, rule, confirmed, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
            params![school_year_id, it.program_id, it.from_parent_id, it.to_parent_id, it.action, rule, note.trim()],
        )?;
        let lid = conn.last_insert_rowid();
        audit::write(
            conn,
            &format!("program.link.{}", it.action),
            "program_link_log",
            Some(lid),
            &format!("세부반 '{}' {}: {} → {}", it.program_name, it.action_label, it.from_parent_name.as_deref().unwrap_or("—"), it.to_parent_name.as_deref().unwrap_or("—")),
            serde_json::json!({ "programId": it.program_id, "from": it.from_parent_id, "to": it.to_parent_id, "rule": rule, "confirmed": true }),
        )?;
        log_ids.push(lid);
        applied += 1;
    }
    Ok(LinkApplyResult { applied, log_ids })
}

/// 통합 프로그램 직접 생성 — 새 프로그램을 만들고 고른 세부반을 잇는다 (하나의 트랜잭션).
pub fn umbrella_create(conn: &Connection, school_year_id: i64, name: &str, member_ids: &[i64], note: &str) -> AppResult<i64> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::invalid("통합 프로그램 이름을 입력해 주세요."));
    }
    let id = match program::resolve_target(conn, school_year_id, n)? {
        Some(program::Target::Program(p)) => {
            let has_parent = parent_of(conn, p)?.is_some();
            if has_parent {
                return Err(AppError::invalid("같은 이름의 프로그램이 이미 다른 통합 프로그램의 세부반입니다."));
            }
            p
        }
        Some(program::Target::Group(_)) => return Err(AppError::invalid("같은 이름의 프로그램 그룹이 있습니다.")),
        None => program::create(conn, school_year_id, n)?,
    };
    let items: Vec<LinkItem> = member_ids.iter().map(|m| LinkItem { program_id: *m, parent_id: Some(id), expected_known: false, expected_parent_id: None }).collect();
    if !items.is_empty() {
        apply(conn, school_year_id, &items, note, "manual")?;
    }
    Ok(id)
}

pub fn history(conn: &Connection, school_year_id: i64) -> AppResult<Vec<LinkLog>> {
    let mut st = conn.prepare(
        "SELECT l.id, l.created_at, l.program_id, p.name, f.name, t.name, l.action, l.rule, l.confirmed, l.note
           FROM program_link_log l JOIN program p ON p.id = l.program_id
           LEFT JOIN program f ON f.id = l.from_parent_id LEFT JOIN program t ON t.id = l.to_parent_id
          WHERE l.school_year_id = ?1 ORDER BY l.id DESC",
    )?;
    let rows = st.query_map([school_year_id], |r| {
        let action: String = r.get(6)?;
        Ok(LinkLog {
            id: r.get(0)?,
            at: r.get(1)?,
            program_id: r.get(2)?,
            program_name: r.get(3)?,
            from_parent_name: r.get(4)?,
            to_parent_name: r.get(5)?,
            action_label: action_label(&action).into(),
            action,
            rule: r.get(7)?,
            confirmed: r.get::<_, i64>(8)? == 1,
            note: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
#[path = "program_link_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "program_link_close_tests.rs"]
mod close_tests;
