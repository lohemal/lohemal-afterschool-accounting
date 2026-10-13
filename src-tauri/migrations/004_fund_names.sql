-- 스키마 v4 (P2.1) — 기본 재원 이름을 용어 기준에 맞춘다. 표 구조는 바뀌지 않는다.
--   수익자 → 일반 수익자, 초3 수익자 부담금(초과금) → 초3 초과금.
-- 사용자가 이미 다른 이름으로 바꿔 둔 경우(시드 이름이 아닌 경우)는 그대로 둔다.
-- 분류는 이름이 아니라 id·parent_id 로 하므로 이름이 무엇이든 집계는 같다.
UPDATE fund_type SET name = '일반 수익자' WHERE code = 'beneficiary' AND name = '수익자';
UPDATE fund_type SET name = '초3 초과금' WHERE code = 'g3_excess' AND name = '초3 수익자 부담금(초과금)';
