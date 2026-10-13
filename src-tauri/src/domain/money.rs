//! 금액 규칙 — 원 단위 정수만 다룬다. 부동소수점은 어디에도 쓰지 않는다.

use crate::error::{AppError, AppResult};

/// 합계. 넘치면 오류로 끝낸다 (조용히 돌아가지 않는다).
pub fn sum(amounts: impl IntoIterator<Item = i64>) -> AppResult<i64> {
    amounts.into_iter().try_fold(0i64, |acc, a| {
        acc.checked_add(a)
            .ok_or_else(|| AppError::new("OVERFLOW", "금액 합계가 표현 범위를 넘었습니다."))
    })
}

/// 사용자가 적은 금액 글자 → 원 단위 정수.
///
/// 허용: `1,234,000` · `-1,234` · `  5000 ` · `₩12,000` · `(3,000)`(회계식 음수) · `12,000원`
/// 거부: 소수점이 있는 값(`3.5`), 숫자가 아닌 글자, 빈 값.
pub fn parse_won(text: &str) -> AppResult<i64> {
    let t = text.trim();
    if t.is_empty() {
        return Err(AppError::invalid("금액이 비어 있습니다."));
    }
    let mut s = t.to_string();
    let mut negative = false;
    if s.starts_with('(') && s.ends_with(')') {
        negative = true;
        s = s[1..s.len() - 1].trim().to_string();
    }
    for sym in ['₩', '원', '\\'] {
        s = s.replace(sym, "");
    }
    let s = s.replace(',', "").replace(' ', "");
    let s = match s.strip_prefix('-') {
        Some(rest) => {
            negative = !negative;
            rest.to_string()
        }
        None => s,
    };
    if s.contains('.') {
        return Err(AppError::invalid("금액은 원 단위 정수여야 합니다 (소수점 불가)."));
    }
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::invalid(format!("금액으로 읽을 수 없습니다: '{t}'")));
    }
    let v: i64 = s
        .parse()
        .map_err(|_| AppError::invalid("금액이 너무 큽니다."))?;
    Ok(if negative { -v } else { v })
}

/// 천 단위 구분 표기. 화면 표시는 TS 쪽 `format.ts` 가 맡고, 이것은 내보내기·시험용.
pub fn format_won(v: i64) -> String {
    let digits = v.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if v < 0 {
        format!("-{out}")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_spellings() {
        assert_eq!(parse_won("1,234,000").unwrap(), 1_234_000);
        assert_eq!(parse_won(" 5000 ").unwrap(), 5000);
        assert_eq!(parse_won("-1,234").unwrap(), -1234);
        assert_eq!(parse_won("₩12,000").unwrap(), 12_000);
        assert_eq!(parse_won("12,000원").unwrap(), 12_000);
        assert_eq!(parse_won("(3,000)").unwrap(), -3000);
        assert_eq!(parse_won("0").unwrap(), 0);
    }

    #[test]
    fn rejects_decimals_and_garbage() {
        assert_eq!(parse_won("3.5").unwrap_err().code, "INVALID");
        assert_eq!(parse_won("12,000.00").unwrap_err().code, "INVALID");
        assert_eq!(parse_won("abc").unwrap_err().code, "INVALID");
        assert_eq!(parse_won("").unwrap_err().code, "INVALID");
    }

    #[test]
    fn sum_detects_overflow() {
        assert_eq!(sum([1, 2, 3]).unwrap(), 6);
        assert_eq!(sum([i64::MAX, 1]).unwrap_err().code, "OVERFLOW");
        // 연간 총액 수준(수십억)은 문제없이 더해진다.
        assert_eq!(sum([2_500_000_000, 2_500_000_000]).unwrap(), 5_000_000_000);
    }

    #[test]
    fn formats_thousands() {
        assert_eq!(format_won(0), "0");
        assert_eq!(format_won(999), "999");
        assert_eq!(format_won(1000), "1,000");
        assert_eq!(format_won(27_329_000), "27,329,000");
        assert_eq!(format_won(-330_850), "-330,850");
    }
}
