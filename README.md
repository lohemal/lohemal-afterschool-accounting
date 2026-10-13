# 방과후 회계 검증 매니저 (AfterSchool Accounting Manager)

방과후학교 회계 자료를 **월별·프로그램별·재원별**로 대조해 징수액과 지출액이 어긋난 위치를 빠르게 찾아 주는
Windows 로컬 데스크톱 프로그램입니다. 모든 차액을 자동으로 해결하는 것이 아니라, 담당자가 직접 확인해야 할
프로그램과 금액을 정확하게 좁혀 주는 것이 1차 목표입니다.

- Tauri 2 · React 18 · TypeScript · Rust · SQLite(rusqlite bundled) · calamine(Excel 읽기)
- 자료는 `%APPDATA%\kr.school.asaccounting\asaccounting.db` 에만 저장되며 외부로 전송되지 않습니다.
- 기존 **방과후 통합 매니저** 와는 별도 프로젝트입니다.

## 현재 단계

**P7-1 — v0.1.0 첫 배포 완료(2026-10-13).** 저장소 [lohemal/lohemal-afterschool-accounting](https://github.com/lohemal/lohemal-afterschool-accounting), Release [v0.1.0](https://github.com/lohemal/lohemal-afterschool-accounting/releases/tag/v0.1.0). 배포·검증 기록은 [docs/07-P6-배포준비.md](docs/07-P6-배포준비.md) 10장, 운영 규칙은 [docs/06-P5-운영.md](docs/06-P5-운영.md). 자유수강권 규칙은 [docs/05-P4-자유수강권.md](docs/05-P4-자유수강권.md) (10장이 P4.1).
정정 규칙은 [docs/04-P3-회계-정정.md](docs/04-P3-회계-정정.md), 설계와 DB 구조는 [docs/01-P0-설계.md](docs/01-P0-설계.md),
업로드·되돌리기 규칙은 [docs/02-P1-업로드와-되돌리기.md](docs/02-P1-업로드와-되돌리기.md),
배분·대조 규칙과 실제 9월 검증 결과는 [docs/03-P2-대조와-배분.md](docs/03-P2-대조와-배분.md) 를 보세요.
외부 배포(push·태그·Release)는 사용자 승인 후 진행합니다. 자동 업데이트 실측(P7-2)은 아직 하지 않았습니다.

## 실행·검사

```bash
npm install
npm run app          # 개발 실행 (vite 1470 + tauri dev) — 실사용 자료 폴더
npm run app:sandbox  # 연습용 자료 폴더(.sandbox/)로 실행 — 실사용 DB 를 건드리지 않음
npm run check        # tsc + vitest + cargo test
# 실제 9월 파일 대조(로컬 전용): ASACCOUNTING_REAL_DIR="..." cargo test --lib real_ -- --ignored --nocapture
py -I scripts/make-fixtures.py   # 가명 시험 엑셀 다시 만들기 (test/fixtures)
npm run app:build    # NSIS 설치본 (서명하려면 TAURI_SIGNING_PRIVATE_KEY=<개인키 내용> TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" 환경변수)
```

## 개인정보 규칙

- 실제 수강생명단·강사료·교재비 파일(`*.xlsx`, `*.xlsm`)은 `.gitignore` 로 저장소에서 제외됩니다.
  시험용 **가명** 자료만 `test/fixtures/` 아래에 둘 수 있습니다.
- DB·백업(backups/*.zip)·로그·작업 이력 파일도 저장소에서 제외됩니다. 백업·내보내기 파일에는 학생 개인정보가 들어 있으니 외부 반출에 주의합니다.
- 로그와 변경 이력에는 학생 이름·금액을 적지 않습니다.
