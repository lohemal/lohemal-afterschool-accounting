//! 자유수강권 사용액 계산 — DB 에 의존하지 않는 순수 규칙.
//!
//! 입력: 지원 한도(연간 · 학기별(이월 여부) · 제한 없음), 이전 학교 사용액, 적용 기간(회계월 차례),
//!       월·프로그램·항목별 수강료(징수 기준액), 담당자가 정한 월 안 배분(선택).
//! 출력: 월·프로그램·항목별 **계산 자유수강권 지원액** = MIN(수강료, 남은 한도) 와 남은 한도 변화.
//!
//! 규칙
//!   · 달 순서대로 쓴다. 한 달 안에 여러 프로그램이 있고 한도가 모자라면 **담당자가 배분을 정해야 한다**
//!     (`needs_decision`). 프로그램 시작일 같은 순서 근거가 자료에 없기 때문이다. 정해 주기 전까지 임시값만 보여 준다.
//!   · 한도가 바닥나면 그 뒤는 지원액 0 (수익자 전환). 한도 초과분을 지원하지 않는다.
//!   · 이전 학교 사용액은 연간이면 연간 한도에서, 학기별이면 전입한 학기의 한도에서 뺀다.
//!   · 학기별 한도의 이월은 1학기에 적용 기간이 걸쳐 있을 때만 1학기 잔액을 2학기에 더한다.
//!   · 수강료 = 자유수강권 + 수익자 + 기타 재원 (이 모듈은 수강료 총액만 받고, 재원별 실제 값과의 차이는 repo 가 낸다).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitSpec {
    /// annual · semester · unlimited
    pub basis: String,
    pub annual: Option<i64>,
    pub s1: Option<i64>,
    pub s2: Option<i64>,
    pub carry_over: bool,
}

/// 한 달·한 프로그램·한 항목의 수강료(징수 기준액 합).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charge {
    pub seq: i32,
    pub program_id: i64,
    pub item_id: i64,
    pub tuition: i64,
}

/// 담당자가 정한 월 안 배분 — 그 칸에 자유수강권으로 얼마를 쓸지.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub seq: i32,
    pub program_id: i64,
    pub item_id: i64,
    pub free_amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub seq: i32,
    pub program_id: i64,
    pub item_id: i64,
    pub tuition: i64,
    /// 계산 자유수강권 지원액
    pub free_expected: i64,
    /// 이 칸 처리 전·후 남은 한도 (제한 없음이면 None)
    pub remaining_before: Option<i64>,
    pub remaining_after: Option<i64>,
    /// 같은 달 여러 프로그램에 한도가 모자라 담당자 배분이 필요함 (값은 임시)
    pub needs_decision: bool,
    /// 담당자 배분을 적용한 값
    pub decided: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub rows: Vec<PlanRow>,
    /// 연간 또는 1·2학기 합 (제한 없음이면 None). 이전 학교 사용액을 빼기 전 값.
    pub limit_total: Option<i64>,
    /// 적용 기간 끝의 남은 한도
    pub remaining_end: Option<i64>,
    pub remaining_s1: Option<i64>,
    pub remaining_s2: Option<i64>,
    /// 한도가 바닥난 달 (차례)
    pub exhausted_seq: Option<i32>,
    pub needs_decision_seqs: Vec<i32>,
    pub notes: Vec<String>,
}

pub fn semester_of(seq: i32) -> i32 {
    if seq <= 6 { 1 } else { 2 }
}

pub fn is_valid_basis(b: &str) -> bool {
    matches!(b, "annual" | "semester" | "unlimited")
}

/// 계산. `window` 는 적용 기간(차례, 양 끝 포함). `prior_seq` 는 이전 학교 사용액이 속한(전입) 달의 차례.
pub fn plan(spec: &LimitSpec, prior_used: i64, prior_seq: i32, window: (i32, i32), charges: &[Charge], decisions: &[Decision]) -> Plan {
    let mut out = Plan::default();
    let mut cs: Vec<Charge> = charges.iter().filter(|c| c.seq >= window.0 && c.seq <= window.1).cloned().collect();
    cs.sort_by_key(|c| (c.seq, c.program_id, c.item_id));
    let prior = prior_used.max(0);
    let unlimited = spec.basis == "unlimited";
    let annual = spec.annual.unwrap_or(0).max(0);
    let s1 = spec.s1.unwrap_or(0).max(0);
    let s2 = spec.s2.unwrap_or(0).max(0);
    out.limit_total = match spec.basis.as_str() {
        "annual" => Some(annual),
        "semester" => Some(s1 + s2),
        _ => None,
    };
    let mut rem_annual = (annual - prior).max(0);
    let mut rem1 = if semester_of(prior_seq) == 1 { (s1 - prior).max(0) } else { s1 };
    let mut rem2 = if semester_of(prior_seq) == 2 { (s2 - prior).max(0) } else { s2 };
    if !unlimited && prior > 0 {
        out.notes.push(format!("이전 학교 사용액 {}원을 {}에서 뺐습니다.", prior, if spec.basis == "annual" { "연간 한도".to_string() } else { format!("{}학기 한도", semester_of(prior_seq)) }));
    }
    let carry = spec.basis == "semester" && spec.carry_over && window.0 <= 6;
    let mut carried = false;
    let mut i = 0;
    while i < cs.len() {
        let seq = cs[i].seq;
        let mut j = i;
        while j < cs.len() && cs[j].seq == seq {
            j += 1;
        }
        let group = &cs[i..j];
        let total: i64 = group.iter().map(|c| c.tuition.max(0)).sum();
        if unlimited {
            for c in group {
                out.rows.push(PlanRow { seq, program_id: c.program_id, item_id: c.item_id, tuition: c.tuition.max(0), free_expected: c.tuition.max(0), remaining_before: None, remaining_after: None, needs_decision: false, decided: false });
            }
            i = j;
            continue;
        }
        let rem: &mut i64 = match spec.basis.as_str() {
            "annual" => &mut rem_annual,
            _ => {
                if semester_of(seq) == 1 {
                    &mut rem1
                } else {
                    if carry && !carried {
                        rem2 += rem1;
                        carried = true;
                        if rem1 > 0 {
                            out.notes.push(format!("1학기 잔액 {}원을 2학기로 이월했습니다.", rem1));
                        }
                    }
                    &mut rem2
                }
            }
        };
        let before_month = *rem;
        // 칸별 지원액 결정
        let mut frees: Vec<i64> = Vec::with_capacity(group.len());
        let mut needs = false;
        let mut decided = false;
        let programs_in_month = {
            let mut ids: Vec<i64> = group.iter().map(|c| c.program_id).collect();
            ids.sort();
            ids.dedup();
            ids.len()
        };
        if total <= *rem {
            frees.extend(group.iter().map(|c| c.tuition.max(0)));
        } else if *rem == 0 {
            frees.extend(group.iter().map(|_| 0));
        } else if programs_in_month == 1 {
            // 한 프로그램이면 항목 순서대로 남은 한도까지만 (담당자 배분 불필요)
            let mut left = *rem;
            for c in group {
                let f = c.tuition.max(0).min(left);
                frees.push(f);
                left -= f;
            }
        } else {
            let ds: Vec<Option<i64>> = group
                .iter()
                .map(|c| decisions.iter().find(|d| d.seq == seq && d.program_id == c.program_id && d.item_id == c.item_id).map(|d| d.free_amount))
                .collect();
            let all = ds.iter().all(|d| d.is_some());
            let sum: i64 = ds.iter().map(|d| d.unwrap_or(0)).sum();
            let each_ok = ds.iter().zip(group.iter()).all(|(d, c)| d.map(|v| v >= 0 && v <= c.tuition.max(0)).unwrap_or(false));
            if all && each_ok && sum == *rem {
                frees.extend(ds.iter().map(|d| d.unwrap()));
                decided = true;
            } else {
                // 임시값: 나열 순서대로 채운다. 담당자가 정하기 전에는 등록에 쓰지 않는다.
                needs = true;
                let mut left = *rem;
                for c in group {
                    let f = c.tuition.max(0).min(left);
                    frees.push(f);
                    left -= f;
                }
                out.needs_decision_seqs.push(seq);
            }
        }
        let mut running = before_month;
        for (c, f) in group.iter().zip(frees.iter()) {
            let after = running - f;
            out.rows.push(PlanRow { seq, program_id: c.program_id, item_id: c.item_id, tuition: c.tuition.max(0), free_expected: *f, remaining_before: Some(running), remaining_after: Some(after), needs_decision: needs, decided });
            running = after;
        }
        *rem = running;
        if before_month > 0 && *rem == 0 && out.exhausted_seq.is_none() && total > 0 {
            out.exhausted_seq = Some(seq);
        }
        i = j;
    }
    if !unlimited {
        match spec.basis.as_str() {
            "annual" => {
                out.remaining_end = Some(rem_annual);
            }
            _ => {
                if carry && !carried && window.1 >= 7 {
                    rem2 += rem1;
                    carried = true;
                }
                out.remaining_s1 = Some(rem1);
                out.remaining_s2 = Some(rem2);
                out.remaining_end = Some(if window.1 >= 7 { rem2 } else { rem1 });
                let _ = carried;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annual(limit: i64) -> LimitSpec {
        LimitSpec { basis: "annual".into(), annual: Some(limit), s1: None, s2: None, carry_over: false }
    }
    fn semester(s1: i64, s2: i64, carry: bool) -> LimitSpec {
        LimitSpec { basis: "semester".into(), annual: None, s1: Some(s1), s2: Some(s2), carry_over: carry }
    }
    fn c(seq: i32, program: i64, tuition: i64) -> Charge {
        Charge { seq, program_id: program, item_id: 1, tuition }
    }

    #[test]
    fn annual_limit_runs_month_by_month_then_converts_to_beneficiary() {
        // 시나리오 2: 한도 300,000 · 이미 280,000 씀(3~9월) · 10월 수강료 50,000 → 자유 20,000 · 수익자 30,000 · 잔액 0
        let charges = vec![c(1, 1, 40_000), c(2, 1, 40_000), c(3, 1, 40_000), c(4, 1, 40_000), c(5, 1, 40_000), c(6, 1, 40_000), c(7, 1, 40_000), c(8, 1, 50_000), c(9, 1, 50_000)];
        let p = plan(&annual(300_000), 0, 1, (1, 12), &charges, &[]);
        let oct = p.rows.iter().find(|r| r.seq == 8).unwrap();
        assert_eq!(oct.free_expected, 20_000);
        assert_eq!(oct.tuition - oct.free_expected, 30_000, "수익자 전환분");
        assert_eq!(oct.remaining_after, Some(0));
        assert_eq!(p.exhausted_seq, Some(8));
        let nov = p.rows.iter().find(|r| r.seq == 9).unwrap();
        assert_eq!(nov.free_expected, 0, "한도가 바닥나면 그 뒤는 전부 수익자");
        assert_eq!(p.remaining_end, Some(0));
        assert!(p.needs_decision_seqs.is_empty());
        let total: i64 = p.rows.iter().map(|r| r.free_expected).sum();
        assert_eq!(total, 300_000, "한도를 넘겨 지원하지 않는다");
    }

    #[test]
    fn window_limits_which_months_count() {
        // 시나리오 1: 3~6월 30,000 씩, 적용 시작 3월 → 4달 모두 지원. 적용 시작이 5월이면 3·4월은 빠진다.
        let charges = vec![c(1, 1, 30_000), c(2, 1, 30_000), c(3, 1, 30_000), c(4, 1, 30_000)];
        let p = plan(&annual(600_000), 0, 1, (1, 12), &charges, &[]);
        assert_eq!(p.rows.iter().map(|r| r.free_expected).sum::<i64>(), 120_000);
        assert_eq!(p.remaining_end, Some(480_000));
        let p = plan(&annual(600_000), 0, 1, (3, 12), &charges, &[]);
        assert_eq!(p.rows.len(), 2);
        assert_eq!(p.rows.iter().map(|r| r.free_expected).sum::<i64>(), 60_000);
    }

    #[test]
    fn unlimited_never_converts() {
        let spec = LimitSpec { basis: "unlimited".into(), annual: None, s1: None, s2: None, carry_over: false };
        let charges = vec![c(1, 1, 900_000), c(2, 1, 900_000)];
        let p = plan(&spec, 0, 1, (1, 12), &charges, &[]);
        assert!(p.rows.iter().all(|r| r.free_expected == r.tuition && r.remaining_after.is_none()));
        assert_eq!(p.limit_total, None);
        assert_eq!(p.exhausted_seq, None);
    }

    #[test]
    fn semester_limits_with_and_without_carry_over() {
        let charges = vec![c(1, 1, 100_000), c(7, 1, 250_000)];
        let no_carry = plan(&semester(200_000, 200_000, false), 0, 1, (1, 12), &charges, &[]);
        assert_eq!(no_carry.rows[0].free_expected, 100_000);
        assert_eq!(no_carry.rows[1].free_expected, 200_000, "1학기 잔액 100,000 은 이월되지 않는다");
        assert_eq!(no_carry.remaining_end, Some(0));
        let carry = plan(&semester(200_000, 200_000, true), 0, 1, (1, 12), &charges, &[]);
        assert_eq!(carry.rows[1].free_expected, 250_000, "이월하면 2학기 한도 300,000 안에서 전액");
        assert_eq!(carry.remaining_end, Some(50_000));
        assert!(carry.notes.iter().any(|n| n.contains("이월")));
        // 2학기부터 대상자면 1학기 잔액은 이월 대상이 아니다
        let late = plan(&semester(200_000, 200_000, true), 0, 7, (7, 12), &charges, &[]);
        assert_eq!(late.rows.len(), 1);
        assert_eq!(late.rows[0].free_expected, 200_000);
    }

    #[test]
    fn prior_school_usage_reduces_the_limit() {
        // 시나리오 4: 연간 600,000 · 이전 학교 250,000 · 9월 전입 · 9월 40,000 → 잔액 350,000 → 310,000
        let charges = vec![c(7, 1, 40_000)];
        let p = plan(&annual(600_000), 250_000, 7, (7, 12), &charges, &[]);
        assert_eq!(p.rows[0].remaining_before, Some(350_000));
        assert_eq!(p.rows[0].free_expected, 40_000);
        assert_eq!(p.remaining_end, Some(310_000));
        // 학기별이면 전입 학기(2학기)에서 뺀다
        let p = plan(&semester(300_000, 300_000, false), 250_000, 7, (7, 12), &charges, &[]);
        assert_eq!(p.rows[0].remaining_before, Some(50_000));
        assert_eq!(p.remaining_s1, Some(300_000));
    }

    #[test]
    fn two_programs_in_one_month_need_a_decision_until_given() {
        // 시나리오 5: 미술 30,000 + 코딩 40,000 · 남은 한도 50,000 → 자유 50,000 / 수익자 20,000, 배분은 담당자가
        let charges = vec![c(5, 10, 30_000), c(5, 20, 40_000)];
        let p = plan(&annual(50_000), 0, 1, (1, 12), &charges, &[]);
        assert_eq!(p.needs_decision_seqs, vec![5]);
        assert!(p.rows.iter().all(|r| r.needs_decision));
        assert_eq!(p.rows.iter().map(|r| r.free_expected).sum::<i64>(), 50_000, "임시값도 한도를 넘지 않는다");
        // 담당자 배분: 미술 10,000 · 코딩 40,000
        let d = vec![Decision { seq: 5, program_id: 10, item_id: 1, free_amount: 10_000 }, Decision { seq: 5, program_id: 20, item_id: 1, free_amount: 40_000 }];
        let p = plan(&annual(50_000), 0, 1, (1, 12), &charges, &d);
        assert!(p.needs_decision_seqs.is_empty());
        assert!(p.rows.iter().all(|r| r.decided && !r.needs_decision));
        assert_eq!(p.rows.iter().find(|r| r.program_id == 10).unwrap().free_expected, 10_000);
        assert_eq!(p.rows.iter().find(|r| r.program_id == 20).unwrap().free_expected, 40_000);
        assert_eq!(p.rows.iter().map(|r| r.tuition - r.free_expected).sum::<i64>(), 20_000, "수익자 20,000");
        // 합이 한도와 다르거나 수강료를 넘는 배분은 받지 않는다
        let bad = vec![Decision { seq: 5, program_id: 10, item_id: 1, free_amount: 30_000 }, Decision { seq: 5, program_id: 20, item_id: 1, free_amount: 30_000 }];
        let p = plan(&annual(50_000), 0, 1, (1, 12), &charges, &bad);
        assert_eq!(p.needs_decision_seqs, vec![5]);
        // 한도가 넉넉하면 배분이 필요 없다
        let p = plan(&annual(500_000), 0, 1, (1, 12), &charges, &[]);
        assert!(p.needs_decision_seqs.is_empty());
        assert_eq!(p.rows.iter().map(|r| r.free_expected).sum::<i64>(), 70_000);
    }

    #[test]
    fn one_program_with_several_items_fills_in_item_order_without_a_decision() {
        let charges = vec![Charge { seq: 7, program_id: 1, item_id: 2, tuition: 21_000 }, Charge { seq: 7, program_id: 1, item_id: 1, tuition: 500_000 }];
        let p = plan(&annual(480_000), 0, 1, (1, 12), &charges, &[]);
        assert!(p.needs_decision_seqs.is_empty());
        assert_eq!(p.rows[0].item_id, 1);
        assert_eq!((p.rows[0].free_expected, p.rows[1].free_expected), (480_000, 0));
    }

    #[test]
    fn negative_tuition_counts_as_zero_and_order_is_by_month() {
        let charges = vec![c(3, 1, 10_000), c(1, 1, -5_000), c(2, 1, 20_000)];
        let p = plan(&annual(25_000), 0, 1, (1, 12), &charges, &[]);
        assert_eq!(p.rows.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(p.rows[0].free_expected, 0);
        assert_eq!(p.rows[1].free_expected, 20_000);
        assert_eq!(p.rows[2].free_expected, 5_000);
    }
}
