//! 학년도 ↔ 회계월. 학년도는 3월에 시작해 다음 해 2월에 끝난다.

/// 학년도의 12개 월을 (차례, 'YYYY-MM') 로 돌려준다. 차례 1 = 3월, 12 = 다음 해 2월.
pub fn months_of(year: i32) -> Vec<(i32, String)> {
    (1..=12)
        .map(|seq| {
            let month = (seq + 1) % 12 + 1; // 1→3, 10→12, 11→1, 12→2
            let y = if seq <= 10 { year } else { year + 1 };
            (seq, format!("{y:04}-{month:02}"))
        })
        .collect()
}

pub fn start_ym(year: i32) -> String {
    format!("{year:04}-03")
}

pub fn end_ym(year: i32) -> String {
    format!("{:04}-02", year + 1)
}

pub fn year_name(year: i32) -> String {
    format!("{year}학년도")
}

/// 'YYYY-MM' → '3월' 같은 짧은 표시.
pub fn label(ym: &str) -> String {
    match ym.get(5..7).and_then(|m| m.parse::<u32>().ok()) {
        Some(m) if (1..=12).contains(&m) => format!("{m}월"),
        _ => ym.to_string(),
    }
}

/// 날짜('YYYY-MM-DD')가 속한 학년도.
pub fn school_year_of(date: &str) -> Option<i32> {
    let y: i32 = date.get(0..4)?.parse().ok()?;
    let m: u32 = date.get(5..7)?.parse().ok()?;
    if !(1..=12).contains(&m) {
        return None;
    }
    Some(if m >= 3 { y } else { y - 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_months_from_march_to_february() {
        let m = months_of(2026);
        assert_eq!(m.len(), 12);
        assert_eq!(m[0], (1, "2026-03".to_string()));
        assert_eq!(m[9], (10, "2026-12".to_string()));
        assert_eq!(m[10], (11, "2027-01".to_string()));
        assert_eq!(m[11], (12, "2027-02".to_string()));
        assert_eq!(start_ym(2026), "2026-03");
        assert_eq!(end_ym(2026), "2027-02");
        assert_eq!(year_name(2026), "2026학년도");
    }

    #[test]
    fn labels_and_year_lookup() {
        assert_eq!(label("2026-03"), "3월");
        assert_eq!(label("2027-02"), "2월");
        assert_eq!(label("garbage"), "garbage");
        assert_eq!(school_year_of("2026-03-01"), Some(2026));
        assert_eq!(school_year_of("2027-02-28"), Some(2026));
        assert_eq!(school_year_of("2027-03-01"), Some(2027));
        assert_eq!(school_year_of("nope"), None);
    }
}
