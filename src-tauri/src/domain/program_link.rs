//! 세부반 표기 규칙 — 순수 함수. '로봇과학A' 에서 통합 프로그램 이름 후보 '로봇과학' 을 뗀다.
//!
//! 끝의 반 구분만 인정한다: 영문 대문자 하나(A·B·C…) 또는 숫자 하나(1~9), 뒤에 '반' 이 붙어도 된다.
//! 바탕 이름은 두 글자 이상이고 마지막 글자가 영문·숫자가 아니어야 한다 — 'ABC' → 'AB' 처럼 영문 이름의
//! 끝 글자를 떼는 것은 세부반이 아니다. 여기서 떼어낸 이름이 **실제로 지급 자료가 있는 프로그램**일 때만
//! 추천이 되고(repo), 확정은 언제나 사용자가 한다.

/// (바탕 이름, 반 구분). 세부반 표기가 아니면 None.
pub fn split_class_suffix(name: &str) -> Option<(String, String)> {
    let t = name.trim();
    let mut chars: Vec<char> = t.chars().collect();
    let mut suffix = String::new();
    if chars.last() == Some(&'반') {
        chars.pop();
        suffix.push('반');
    }
    let last = *chars.last()?;
    let is_class = last.is_ascii_uppercase() || ('1'..='9').contains(&last);
    if !is_class {
        return None;
    }
    chars.pop();
    suffix.insert(0, last);
    while matches!(chars.last(), Some(' ') | Some('-') | Some('_') | Some('·')) {
        chars.pop();
    }
    if chars.len() < 2 {
        return None;
    }
    let base_last = *chars.last()?;
    if base_last.is_ascii_alphanumeric() {
        return None;
    }
    Some((chars.into_iter().collect(), suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_suffix_is_recognized_only_after_a_real_name() {
        assert_eq!(split_class_suffix("로봇과학A"), Some(("로봇과학".into(), "A".into())));
        assert_eq!(split_class_suffix("만화애니메이션B"), Some(("만화애니메이션".into(), "B".into())));
        assert_eq!(split_class_suffix("토탈공예미니어처1"), Some(("토탈공예미니어처".into(), "1".into())));
        assert_eq!(split_class_suffix("영어 A"), Some(("영어".into(), "A".into())));
        assert_eq!(split_class_suffix("코딩A반"), Some(("코딩".into(), "A반".into())));
        assert_eq!(split_class_suffix("코딩-2반"), Some(("코딩".into(), "2반".into())));
        assert_eq!(split_class_suffix(" 금요방송댄스B "), Some(("금요방송댄스".into(), "B".into())));
    }

    #[test]
    fn plain_names_and_english_endings_are_not_classes() {
        assert_eq!(split_class_suffix("ABC"), None, "영문 이름의 끝 글자는 반 구분이 아니다");
        assert_eq!(split_class_suffix("코딩AB"), None);
        assert_eq!(split_class_suffix("한자"), None);
        assert_eq!(split_class_suffix("A"), None);
        assert_eq!(split_class_suffix("가A"), None, "바탕 이름은 두 글자 이상");
        assert_eq!(split_class_suffix("로봇과학a"), None, "소문자는 인정하지 않는다");
        assert_eq!(split_class_suffix("코딩0"), None);
        assert_eq!(split_class_suffix(""), None);
    }
}
