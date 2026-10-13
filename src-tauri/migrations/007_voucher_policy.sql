-- ============================================================================
-- 스키마 v7 (P4.1) — 자유수강권 정책 보완: 학교별 학기 기간 · 지원 대상 항목
--
--   · voucher_policy 에 학기 시작·종료일 네 칸을 더한다. NULL 이면 기본값(1학기 3/1~8/31, 2학기 9/1~다음 해 2월 말)으로 읽는다.
--     기존 v6 정책 행은 그대로 보존되고 기본값으로 해석되므로 계산 결과가 바뀌지 않는다.
--   · voucher_policy_item: 정책 판마다 지원 대상 항목. 행이 하나도 없으면 모든 항목 지원(P4 동작과 호환).
--     정책이 바뀌면 새 정책 행과 함께 새로 적히므로 과거 정책의 적용 기준을 그대로 확인할 수 있다.
--   원장·정정·대상자·묶음·환불 표는 건드리지 않는다.
-- ============================================================================

ALTER TABLE voucher_policy ADD COLUMN semester1_from TEXT;
ALTER TABLE voucher_policy ADD COLUMN semester1_to   TEXT;
ALTER TABLE voucher_policy ADD COLUMN semester2_from TEXT;
ALTER TABLE voucher_policy ADD COLUMN semester2_to   TEXT;

CREATE TABLE voucher_policy_item (
  policy_id       INTEGER NOT NULL REFERENCES voucher_policy (id),
  amount_item_id  INTEGER NOT NULL REFERENCES amount_item (id),
  eligible        INTEGER NOT NULL CHECK (eligible IN (0, 1)),
  PRIMARY KEY (policy_id, amount_item_id)
);

-- 정책 판의 항목 설정은 이력이다. 고치거나 지우지 않는다 (바꾸려면 새 정책을 저장한다).
CREATE TRIGGER trg_voucher_policy_item_no_delete BEFORE DELETE ON voucher_policy_item BEGIN SELECT RAISE(ABORT, 'VOUCHER_NO_DELETE'); END;
CREATE TRIGGER trg_voucher_policy_item_immutable BEFORE UPDATE ON voucher_policy_item BEGIN SELECT RAISE(ABORT, 'VOUCHER_IMMUTABLE'); END;
