-- ============================================================================
-- 스키마 v10 (P7-1.1) — 세부반 → 통합 프로그램 연결
--
--   학생 징수자료는 세부반('로봇과학A'·'로봇과학B') 단위, 강사료·교재재료비 지출은 프로그램('로봇과학') 단위다.
--   둘 다 program 행으로 남기고(원본 표기·원장 불변), 세부반이 어느 통합 프로그램에 속하는지를 이 표가 잇는다.
--   · 세부반 하나는 통합 프로그램 하나에만 (UNIQUE program_id). 통합 프로그램 하나에 세부반 여럿.
--   · 깊이는 한 단계 — 통합 프로그램은 다시 세부반이 될 수 없다 (repo 가 검사).
--   · 여기 있는 행은 모두 사용자가 확정한 것이다. 추천은 저장하지 않고 매번 계산한다.
--   · 대조(recon)는 세부반 칸을 통합 프로그램 칸에 합산해 지급과 비교한다. 원장 거래는 바꾸지 않는다.
--   · 변경 이력(program_link_log)은 지우지 않는다.
-- ============================================================================

CREATE TABLE program_link (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  program_id      INTEGER NOT NULL REFERENCES program (id) ON DELETE CASCADE,   -- 세부반
  parent_id       INTEGER NOT NULL REFERENCES program (id) ON DELETE CASCADE,   -- 통합 프로그램
  rule            TEXT    NOT NULL DEFAULT 'manual',   -- suggested(추천을 확정) · manual(직접 선택)
  note            TEXT    NOT NULL DEFAULT '',
  created_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  updated_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (program_id),
  CHECK (program_id <> parent_id)
);
CREATE INDEX ix_program_link_parent ON program_link (parent_id);

CREATE TABLE program_link_log (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  program_id      INTEGER NOT NULL REFERENCES program (id),
  from_parent_id  INTEGER REFERENCES program (id),
  to_parent_id    INTEGER REFERENCES program (id),
  action          TEXT    NOT NULL CHECK (action IN ('confirm', 'change', 'unlink')),
  rule            TEXT    NOT NULL DEFAULT '',
  confirmed       INTEGER NOT NULL DEFAULT 1 CHECK (confirmed IN (0, 1)),   -- 사용자 확인을 거친 변경인가 (항상 1)
  note            TEXT    NOT NULL DEFAULT '',
  created_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);
CREATE INDEX ix_program_link_log_program ON program_link_log (program_id, id);

CREATE TRIGGER trg_program_link_log_no_delete
BEFORE DELETE ON program_link_log
BEGIN
  SELECT RAISE(ABORT, 'LINK_LOG_NO_DELETE');
END;
