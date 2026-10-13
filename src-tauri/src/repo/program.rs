//! 프로그램 · 별칭 · 그룹. 업로드 파일마다 표기가 달라('코딩A' / '코딩' / '우쿨렐레&통기타')
//! 대표 프로그램이나 그룹에 별칭을 잇는다. **비슷하다는 이유로 자동으로 합치지 않는다** —
//! 제안만 하고 사용자가 확정하면 규칙(별칭)으로 저장해 다음 업로드에 재사용한다.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::model::{Program, ProgramGroup, ProgramMapInput};
use crate::repo::audit;

pub fn list(conn: &Connection, school_year_id: i64) -> AppResult<Vec<Program>> {
    let mut st = conn.prepare(
        "SELECT p.id, p.school_year_id, p.name, p.note, p.is_active,
                (SELECT count(*) FROM ledger_entry e WHERE e.program_id = p.id AND e.state = 'active'),
                l.parent_id, pp.name,
                (SELECT count(*) FROM program_link c WHERE c.parent_id = p.id)
           FROM program p LEFT JOIN program_link l ON l.program_id = p.id LEFT JOIN program pp ON pp.id = l.parent_id
          WHERE p.school_year_id = ?1 ORDER BY p.name",
    )?;
    let rows = st.query_map([school_year_id], |r| {
        Ok(Program {
            id: r.get(0)?,
            school_year_id: r.get(1)?,
            name: r.get(2)?,
            note: r.get(3)?,
            is_active: r.get::<_, i64>(4)? == 1,
            aliases: Vec::new(),
            entry_count: r.get(5)?,
            parent_id: r.get(6)?,
            parent_name: r.get(7)?,
            child_count: r.get(8)?,
        })
    })?;
    let mut out: Vec<Program> = rows.collect::<Result<_, _>>()?;
    let mut st = conn.prepare("SELECT alias FROM program_alias WHERE program_id = ?1 ORDER BY alias")?;
    for p in out.iter_mut() {
        let rows = st.query_map([p.id], |r| r.get::<_, String>(0))?;
        p.aliases = rows.collect::<Result<_, _>>()?;
    }
    Ok(out)
}

pub fn name_of(conn: &Connection, id: i64) -> AppResult<String> {
    conn.query_row("SELECT name FROM program WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::not_found("프로그램을 찾지 못했습니다."))
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
    audit::write(conn, "program.create", "program", Some(id), &format!("프로그램 '{n}' 등록"), serde_json::json!({}))?;
    Ok(id)
}

pub fn add_alias(conn: &Connection, school_year_id: i64, program_id: i64, alias: &str) -> AppResult<i64> {
    let a = alias.trim();
    if a.is_empty() {
        return Err(AppError::invalid("별칭을 입력해 주세요."));
    }
    conn.execute(
        "INSERT INTO program_alias (school_year_id, program_id, alias) VALUES (?1, ?2, ?3)
         ON CONFLICT(school_year_id, alias) DO UPDATE SET program_id = excluded.program_id, program_group_id = NULL",
        params![school_year_id, program_id, a],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn add_group_alias(conn: &Connection, school_year_id: i64, group_id: i64, alias: &str) -> AppResult<i64> {
    let a = alias.trim();
    if a.is_empty() {
        return Err(AppError::invalid("별칭을 입력해 주세요."));
    }
    conn.execute(
        "INSERT INTO program_alias (school_year_id, program_group_id, alias) VALUES (?1, ?2, ?3)
         ON CONFLICT(school_year_id, alias) DO UPDATE SET program_group_id = excluded.program_group_id, program_id = NULL",
        params![school_year_id, group_id, a],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_alias(conn: &Connection, school_year_id: i64, alias: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM program_alias WHERE school_year_id = ?1 AND alias = ?2",
        params![school_year_id, alias.trim()],
    )?;
    Ok(())
}

// ── 그룹 ────────────────────────────────────────────────────────────────

pub fn groups(conn: &Connection, school_year_id: i64) -> AppResult<Vec<ProgramGroup>> {
    let mut st = conn.prepare(
        "SELECT id, school_year_id, name, note FROM program_group WHERE school_year_id = ?1 ORDER BY name",
    )?;
    let rows = st.query_map([school_year_id], |r| {
        Ok(ProgramGroup {
            id: r.get(0)?,
            school_year_id: r.get(1)?,
            name: r.get(2)?,
            note: r.get(3)?,
            member_ids: Vec::new(),
            member_names: Vec::new(),
        })
    })?;
    let mut out: Vec<ProgramGroup> = rows.collect::<Result<_, _>>()?;
    let mut st = conn.prepare(
        "SELECT p.id, p.name FROM program_group_member m JOIN program p ON p.id = m.program_id
          WHERE m.group_id = ?1 ORDER BY p.name",
    )?;
    for g in out.iter_mut() {
        let rows = st.query_map([g.id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, name) = row?;
            g.member_ids.push(id);
            g.member_names.push(name);
        }
    }
    Ok(out)
}

pub fn group_name_of(conn: &Connection, id: i64) -> AppResult<String> {
    conn.query_row("SELECT name FROM program_group WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::not_found("프로그램 그룹을 찾지 못했습니다."))
}

pub fn group_create(conn: &Connection, school_year_id: i64, name: &str, member_ids: &[i64]) -> AppResult<i64> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::invalid("그룹 이름을 입력해 주세요."));
    }
    if member_ids.len() < 2 {
        return Err(AppError::invalid("그룹에는 프로그램이 둘 이상 있어야 합니다."));
    }
    conn.execute(
        "INSERT INTO program_group (school_year_id, name) VALUES (?1, ?2)",
        params![school_year_id, n],
    )?;
    let id = conn.last_insert_rowid();
    for m in member_ids {
        let same_year: i64 = conn.query_row(
            "SELECT count(*) FROM program WHERE id = ?1 AND school_year_id = ?2",
            params![m, school_year_id],
            |r| r.get(0),
        )?;
        if same_year == 0 {
            return Err(AppError::invalid("다른 학년도의 프로그램은 그룹에 넣을 수 없습니다."));
        }
        if crate::repo::program_link::parent_of(conn, *m)?.is_some() {
            return Err(AppError::invalid(format!("'{}' 은(는) 통합 프로그램의 세부반이라 그룹에 넣을 수 없습니다 (지급 중복 방지).", name_of(conn, *m)?)));
        }
        conn.execute(
            "INSERT OR IGNORE INTO program_group_member (group_id, program_id) VALUES (?1, ?2)",
            params![id, m],
        )?;
    }
    audit::write(conn, "program.group.create", "program_group", Some(id), &format!("프로그램 그룹 '{n}' ({}개) 등록", member_ids.len()), serde_json::json!({}))?;
    Ok(id)
}

// ── 해석 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Program(i64),
    Group(i64),
}

/// 대표 이름 또는 별칭으로 찾는다. 없으면 None (추측하지 않는다).
pub fn resolve_target(conn: &Connection, school_year_id: i64, text: &str) -> AppResult<Option<Target>> {
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
    if let Some(id) = by_name {
        return Ok(Some(Target::Program(id)));
    }
    let alias: Option<(Option<i64>, Option<i64>)> = conn
        .query_row(
            "SELECT program_id, program_group_id FROM program_alias WHERE school_year_id = ?1 AND alias = ?2",
            params![school_year_id, t],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match alias {
        Some((Some(p), _)) => Some(Target::Program(p)),
        Some((_, Some(g))) => Some(Target::Group(g)),
        _ => {
            // 그룹 이름 자체
            conn.query_row(
                "SELECT id FROM program_group WHERE school_year_id = ?1 AND name = ?2",
                params![school_year_id, t],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(Target::Group)
        }
    })
}

/// 호환용 — 프로그램 id 만.
pub fn resolve(conn: &Connection, school_year_id: i64, text: &str) -> AppResult<Option<i64>> {
    Ok(match resolve_target(conn, school_year_id, text)? {
        Some(Target::Program(p)) => Some(p),
        _ => None,
    })
}

/// 업로드 미리보기의 제안 — 반 구분을 뗀 이름의 프로그램이 있으면 **제안**한다. 확정은 사용자가 한다.
/// (프로그램 id, 이름, 종류, 근거). 종류 `child` = 세부반으로 등록해 그 통합 프로그램에 연결(지급 자료가 있거나 이미 통합 프로그램일 때),
/// `alias` = 표기만 잇기(그 밖의 경우 — 세부반 구분이 필요 없을 때 고른다).
pub fn suggest(conn: &Connection, school_year_id: i64, text: &str) -> AppResult<Option<(i64, String, String, String)>> {
    let Some((base, _)) = crate::domain::program_link::split_class_suffix(text) else {
        return Ok(None);
    };
    match resolve_target(conn, school_year_id, &base)? {
        Some(Target::Program(id)) => {
            let name = name_of(conn, id)?;
            if crate::repo::program_link::parent_of(conn, id)?.is_some() {
                return Ok(None);
            }
            let children = crate::repo::program_link::children_of(conn, id)?.len();
            let exp: i64 = conn.query_row("SELECT count(*) FROM ledger_entry WHERE program_id = ?1 AND state = 'active' AND flow = 'expenditure'", [id], |r| r.get(0))?;
            if children > 0 {
                Ok(Some((id, name.clone(), "child".into(), format!("'{name}' 은(는) 이미 세부반 {children}개를 가진 통합 프로그램"))))
            } else if exp > 0 {
                Ok(Some((id, name.clone(), "child".into(), format!("'{name}' 에 지급 자료 {exp}건 — 세부반 징수를 합산해 대조"))))
            } else {
                Ok(Some((id, name.clone(), "alias".into(), format!("'{name}' 에 아직 지급 자료가 없음"))))
            }
        }
        _ => Ok(None),
    }
}

/// 매핑 저장 — 별칭을 기존 프로그램·그룹에 잇거나, 새 프로그램·그룹을 만들어 잇는다.
pub fn map_save(conn: &Connection, input: &ProgramMapInput) -> AppResult<Target> {
    let alias = input.alias.trim();
    if alias.is_empty() {
        return Err(AppError::invalid("표기를 입력해 주세요."));
    }
    let target = match input.target.as_str() {
        "program" => {
            let id = input.program_id.ok_or_else(|| AppError::invalid("연결할 프로그램을 골라 주세요."))?;
            let name = name_of(conn, id)?;
            if name != alias {
                add_alias(conn, input.school_year_id, id, alias)?;
            }
            Target::Program(id)
        }
        "group" => {
            let id = input.group_id.ok_or_else(|| AppError::invalid("연결할 그룹을 골라 주세요."))?;
            let name = group_name_of(conn, id)?;
            if name != alias {
                add_group_alias(conn, input.school_year_id, id, alias)?;
            }
            Target::Group(id)
        }
        "new_program" => {
            let name = input.new_name.clone().unwrap_or_else(|| alias.to_string());
            let id = match resolve_target(conn, input.school_year_id, name.trim())? {
                Some(Target::Program(p)) => p,
                Some(Target::Group(_)) => return Err(AppError::invalid("같은 이름의 그룹이 이미 있습니다.")),
                None => create(conn, input.school_year_id, &name)?,
            };
            if name.trim() != alias {
                add_alias(conn, input.school_year_id, id, alias)?;
            }
            Target::Program(id)
        }
        "new_group" => {
            let name = input.new_name.clone().unwrap_or_else(|| alias.to_string());
            let members = input.member_ids.clone().unwrap_or_default();
            let id = group_create(conn, input.school_year_id, &name, &members)?;
            if name.trim() != alias {
                add_group_alias(conn, input.school_year_id, id, alias)?;
            }
            Target::Group(id)
        }
        // 세부반으로 등록하고 통합 프로그램에 연결(확정). 원본 표기는 세부반 이름으로 남는다.
        "new_child" => {
            let parent = input.parent_id.ok_or_else(|| AppError::invalid("연결할 통합 프로그램을 골라 주세요."))?;
            let name = input.new_name.clone().unwrap_or_else(|| alias.to_string());
            let id = match resolve_target(conn, input.school_year_id, name.trim())? {
                Some(Target::Program(p)) => p,
                Some(Target::Group(_)) => return Err(AppError::invalid("같은 이름의 그룹이 이미 있습니다.")),
                None => create(conn, input.school_year_id, &name)?,
            };
            if name.trim() != alias {
                add_alias(conn, input.school_year_id, id, alias)?;
            }
            crate::repo::program_link::apply(
                conn,
                input.school_year_id,
                &[crate::model::LinkItem { program_id: id, parent_id: Some(parent), expected_known: false, expected_parent_id: None }],
                "업로드 미리보기에서 확정",
                "suggested",
            )?;
            Target::Program(id)
        }
        other => return Err(AppError::invalid(format!("알 수 없는 매핑 종류: {other}"))),
    };
    audit::write(
        conn,
        "program.map",
        "program_alias",
        None,
        &format!("프로그램 표기 '{alias}' 매핑 저장"),
        serde_json::json!({ "target": input.target }),
    )?;
    Ok(target)
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
        // 같은 별칭을 다른 프로그램에 다시 걸면 옮겨 간다 (매핑 수정).
        let robot = db.write(|tx| create(tx, y.id, "로봇과학")).unwrap();
        db.write(|tx| add_alias(tx, y.id, robot, "코딩A")).unwrap();
        assert_eq!(db.read(|c| resolve(c, y.id, "코딩A")).unwrap(), Some(robot));
        let list = db.read(|c| list(c, y.id)).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].aliases, vec!["코딩B"]);
    }

    #[test]
    fn suggestion_is_only_a_suggestion() {
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| year::create(tx, 2026)).unwrap();
        let coding = db.write(|tx| create(tx, y.id, "코딩")).unwrap();
        let s = db.read(|c| suggest(c, y.id, "코딩A")).unwrap().expect("제안");
        assert_eq!((s.0, s.1.as_str(), s.2.as_str()), (coding, "코딩", "alias"), "지급 자료가 없으면 표기 잇기 제안");
        assert_eq!(db.read(|c| resolve(c, y.id, "코딩A")).unwrap(), None, "제안은 자동 적용되지 않는다");
        assert_eq!(db.read(|c| suggest(c, y.id, "한자")).unwrap(), None);
        assert_eq!(db.read(|c| suggest(c, y.id, "A")).unwrap(), None);
    }

    #[test]
    fn group_alias_maps_to_group_not_a_program() {
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| year::create(tx, 2026)).unwrap();
        let uk = db.write(|tx| create(tx, y.id, "우쿨렐레")).unwrap();
        let gt = db.write(|tx| create(tx, y.id, "통기타")).unwrap();
        let input = ProgramMapInput {
            school_year_id: y.id,
            alias: "우쿨렐레&통기타".into(),
            target: "new_group".into(),
            program_id: None,
            group_id: None,
            new_name: None,
            member_ids: Some(vec![uk, gt]),
            parent_id: None,
        };
        let t = db.write(|tx| map_save(tx, &input)).unwrap();
        let Target::Group(gid) = t else { panic!("그룹이어야 한다") };
        assert_eq!(db.read(|c| resolve_target(c, y.id, "우쿨렐레&통기타")).unwrap(), Some(Target::Group(gid)));
        assert_eq!(db.read(|c| resolve(c, y.id, "우쿨렐레&통기타")).unwrap(), None);
        let gs = db.read(|c| groups(c, y.id)).unwrap();
        assert_eq!(gs[0].member_names, vec!["우쿨렐레", "통기타"]);
        // 멤버 하나짜리 그룹은 안 된다.
        let bad = ProgramMapInput { member_ids: Some(vec![uk]), alias: "x".into(), new_name: Some("x".into()), ..input.clone() };
        assert_eq!(db.write(|tx| map_save(tx, &bad)).unwrap_err().code, "INVALID");
        // new_program 으로 '한자' 를 만들며 '한자급수' 를 별칭으로.
        let np = ProgramMapInput {
            school_year_id: y.id, alias: "한자급수".into(), target: "new_program".into(),
            program_id: None, group_id: None, new_name: Some("한자".into()), member_ids: None, parent_id: None,
        };
        let Target::Program(hid) = db.write(|tx| map_save(tx, &np)).unwrap() else { panic!() };
        assert_eq!(db.read(|c| resolve(c, y.id, "한자급수")).unwrap(), Some(hid));
        assert_eq!(db.read(|c| resolve(c, y.id, "한자")).unwrap(), Some(hid));
    }
}
