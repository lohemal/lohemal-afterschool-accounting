"""시험용 가명 엑셀 7종 — 실제 양식과 같은 시트명·머리글·열 배치, 내용은 전부 가상.

실행: py -I scripts/make-fixtures.py
결과: test/fixtures/*.xlsx · *.xlsm (저장소에 포함, 개인정보 없음)

실제 파일 구조는 docs/01-P0-설계.md 2장의 실측을 따른다. 금액은 Rust 시험이
기대값으로 쓰므로 바꾸면 src-tauri/src/excel/tests.rs 도 함께 고친다.
"""
import os, sys
from openpyxl import Workbook

sys.stdout.reconfigure(encoding='utf-8')
OUT = os.path.join(os.path.dirname(__file__), '..', 'test', 'fixtures')
os.makedirs(OUT, exist_ok=True)

STUDENTS = [  # 학년, 반, 번호, 가명
    (1, '가람', 3, '가명A'), (1, '나리', 7, '가명B'), (2, '다솜', 1, '가명C'),
    (3, '가람', 5, '가명D'), (3, '나리', 9, '가명E'), (4, '다솜', 2, '가명F'),
    (5, '라온', 4, '가명G'), (6, '가람', 8, '가명H'),
]

def save(wb, name):
    p = os.path.join(OUT, name)
    wb.save(p)
    print('saved', name)

# ── E. 통합 매니저 수강생명단 ───────────────────────────────────────────────
wb = Workbook(); ws = wb.active; ws.title = '수강생명단'
ws.append(['부서', '학년', '반', '번호', '이름', '지원유형', '강사료', '수용비', '교재비', '재료비', '합계', '상태', '변경사유'])
rows = [
    ('코딩A', *STUDENTS[0], '일반', 35000, 1500, 14000, 0, '수강중', ''),
    ('코딩A', *STUDENTS[1], '방과후 이용권', 35000, 1500, 14000, 0, '수강중', ''),
    ('코딩B', *STUDENTS[2], '자유수강권', 35000, 1500, 14000, 0, '수강중', ''),
    ('로봇과학A', *STUDENTS[3], '일반', 42000, 1800, 9000, 51000, '수강중', ''),
    ('로봇과학A', *STUDENTS[4], '방과후 이용권 + 자유수강권', 42000, 1800, 9000, 51000, '수강중', ''),
    ('로봇과학A', *STUDENTS[5], '일반', 42000, 1800, 9000, 51000, '취소', '9.4.취소'),
    ('금요방송댄스A', *STUDENTS[6], '일반', 0, 0, 0, 0, '수강중', ''),
    ('항공드론A', *STUDENTS[7], '일반', 40000, 1200, 0, 80000, '수강중', ''),
]
for r in rows:
    dept, g, c, n, name, typ, a, b, cc, d, st, why = r
    ws.append([dept, str(g), c, n, name, typ, a, b, cc, d, a + b + cc + d, st, why])
save(wb, 'roster_manager.xlsx')

# ── F. 강사료 지급 내역 ─────────────────────────────────────────────────────
wb = Workbook(); ws = wb.active; ws.title = '강사료내역'
ws['A1'] = '2026학년도 9월  강사료 내역'
ws['D2'] = 'A반'; ws['E2'] = 'B반'; ws['J2'] = '(기간: 2026. 8. 20. ~ 9. 30.)'
ws.merge_cells('E2:H2'); ws.merge_cells('J2:K2')
ws.append(['순번', '부서', '강사명', '맞춤형지원\n(시간당40,000원)', '3학년 지원금', '3학년 수익자', '수익자징수', '자유수강권', '최종강사료합계', '계좌번호', '은행'])
pay = [
    (1, '코딩', '강사가명1', 400000, 315000, 0, 210000, 70000),
    (2, '로봇과학', '강사가명2', 480000, 546000, 0, 294000, 0),
    (3, '우쿨렐레&통기타', '강사가명3', 480000, 0, 0, 288000, 96000),
    (4, '항공드론', '강사가명4', 400000, 280000, 11100, 420000, 0),
]
for i, (n, dept, name, a, b, c, d, e) in enumerate(pay):
    r = 4 + i
    ws.append([n, dept, name, a, b, c, d, e, None, '', ''])
    ws.cell(r, 9).value = f'=SUM(D{r}:H{r})'   # 수식 — openpyxl 은 계산값을 저장하지 않는다
tot = 4 + len(pay)
ws.cell(tot, 3).value = '합계'
for col in range(4, 10):
    ws.cell(tot, col).value = sum(p[col - 1] for p in pay) if col < 9 else sum(sum(p[3:8]) for p in pay)
save(wb, 'instructor_pay_formula.xlsx')
# 계산값이 들어 있는 판 (일반적인 경우)
for i, p in enumerate(pay):
    ws.cell(4 + i, 9).value = sum(p[3:8])
save(wb, 'instructor_pay.xlsx')

# ── G. 교재·재료비 지급업체 ────────────────────────────────────────────────
wb = Workbook(); ws = wb.active; ws.title = '2026.9.'
ws['B1'] = '2026학년도 9월 -교재,재료비 지급 업체 명단 '
ws.merge_cells('B1:J1')
ws.append([])
ws.append(['순번', '부서명', '수익자징수금액', '3학년 지원금', '3학년 수익자', '자유수강권', '최종지급금액', '업체명', '연락처', '지급업체 메일주소'])
vend = [
    (1, '코딩', 224000, 126000, 0, 28000, '업체가명1'),
    (2, '로봇과학', 720000, 176900, 3100, 0, '업체가명2'),
    (3, None, 90000, 266900, 3100, 0, '업체가명3'),      # B5:B6 병합 → 로봇과학 이어받음
    (4, '체스', 196000, 0, 0, 14000, '업체가명4'),
    (5, None, 72000, 48000, 0, 0, '업체가명5'),          # 병합 없음 → 확인 필요
    (6, '주산암산', 40000, 18000, 0, 10000, '업체가명6'),
    (7, None, 0, 0, 0, 0, '업체가명7'),                  # B9:B10 병합, 0원 행
    (8, '통기타', 0, 0, 0, 0, '업체가명8'),
]
for i, (n, dept, a, b, c, d, v) in enumerate(vend):
    ws.append([n, dept, a, b, c, d, a + b + c + d, v, '010-0000-0000', 'x@example.com'])
ws.merge_cells('B5:B6'); ws.merge_cells('B9:B10')
tot = 4 + len(vend)
ws.cell(tot, 2).value = '합계'
for col in range(3, 8):
    ws.cell(tot, col).value = sum((p[col - 1] if col < 7 else sum(p[2:6])) for p in vend)
ws.merge_cells(start_row=tot, start_column=8, end_row=tot, end_column=10)
ws.cell(tot + 2, 3).value = '교재비'; ws.cell(tot + 2, 4).value = '재료비'
ws.cell(tot + 3, 3).value = 500000; ws.cell(tot + 3, 4).value = 1500000
save(wb, 'vendor_pay.xlsx')

# ── C. 징수명단(기안용) ───────────────────────────────────────────────────
wb = Workbook()
def fund_sheet(ws, title, rows, totals_block=True):
    ws['A1'] = title; ws.merge_cells('A1:I1')
    ws.append([])
    ws.append(['학년', '반', '번호', '이름', '강사료', '수용비', '교재비', '재료비', '합계', None, '강사료', '수용비', '교재비', '재료비', '합계'])
    for (g, c, n, name, a, b, cc, d) in rows:
        ws.append([g, c, n, name, a, b, cc, d, a + b + cc + d])
    if totals_block:
        ws['K4'] = sum(r[4] for r in rows); ws['L4'] = sum(r[5] for r in rows)
        ws['M4'] = sum(r[6] for r in rows); ws['N4'] = sum(r[7] for r in rows)
        ws['O4'] = sum(sum(r[4:8]) for r in rows)
ben = [(*STUDENTS[0], 35000, 1500, 14000, 0), (*STUDENTS[2], 35000, 1500, 14000, 0), (*STUDENTS[5], 42000, 1800, 9000, 51000)]
free = [(*STUDENTS[6], 77000, 3300, 0, 40000)]
ws = wb.active; ws.title = '전체'
fund_sheet(ws, '2026학년도 9월 전체', ben + free + [(*STUDENTS[1], 35000, 1500, 14000, 0), (*STUDENTS[3], 42000 + 5000, 1800, 9000, 51000)])
ws.append([None, None, None, '합계', 0, 0, 0, 0, 0])  # 합계 행은 건너뛰어야 한다
fund_sheet(wb.create_sheet('수익자'), '2026학년도 9월 수익자', ben)
ws = wb.create_sheet('방과후이용권')
ws['A1'] = '2026학년도 9월 방과후 이용권'; ws.merge_cells('A1:N1'); ws.append([])
ws.append(['학년', '반', '번호', '이름', '이용권 강사료', '이용권 수용비', '이용권 교재비', '이용권 재료비', '이용권 합계',
           '초과 강사료', '초과 수용비', '초과 교재비', '초과 재료비', '초과 합계', None, '항목', '강사료', '수용비', '교재비', '재료비', '합계'])
ws.append([*STUDENTS[1], 35000, 1500, 14000, 0, 50500, 0, 0, 0, 0, 0])
ws.append([*STUDENTS[3], 42000, 1800, 9000, 51000, 103800, 5000, 0, 0, 0, 5000])
ws = wb.create_sheet('자유수강권')
fund_sheet(ws, '2026학년도 9월 자유수강권', free)
ws.append([None, None, None, '합계', 77000, 3300, 0, 40000, 120300])
save(wb, 'collect_initial.xlsx')
# 2판 — 수익자 시트 첫 학생의 강사료만 35,000 → 30,000 (합계 479,400). 교체·복원 시험용.
wb['수익자']['E4'] = 30000; wb['수익자']['I4'] = 30000 + 1500 + 14000
wb['전체']['E4'] = 30000; wb['전체']['I4'] = 30000 + 1500 + 14000
save(wb, 'collect_initial_v2.xlsx')

# ── D. 환불 및 추가징수 명단 ──────────────────────────────────────────────
wb = Workbook(); ws = wb.active; ws.title = '환불'
ws.append(['순번', '부서', '학년', '반', '번호', '이름', '강사료', '수용비', '교재비', '재료비', '합계', '비고'])
ws.append([1, '로봇과학A', *STUDENTS[3], None, None, 13500, 76500, 90000, ''])
ws.append([2, '바이올린B', *STUDENTS[2], 24000, 900, None, None, 24900, ''])
ws.append([None, None, None, None, None, '합계', 24000, 900, 13500, 76500, 114900])
ws = wb.create_sheet('추가징수')
ws.append(['순번', '부서', '학년', '반', '번호', '이름', '강사료', '수용비', '교재비', '재료비', '합계'])
ws.append([1, '컴퓨터C', *STUDENTS[6], 31500, 1350, 18000, None, 50850])
ws.append([None, None, None, None, None, '합계', 31500, 1350, 18000, 0, 50850])
save(wb, 'refund_extra_s1.xlsx')

# ── B. 1학기 정산 명단2 (xlsm) ─────────────────────────────────────────────
wb = Workbook()
ws = wb.active; ws.title = '요약'
ws.append([None, None, '징수', None, None, None, None, '환불', None, None, None, None, '추가징수', None, None, None, None, '총합', None, None, None, '비고'])
ws.append(['순번', '부서', '강사료', '수용비', '교재비', '재료비', '합계', '강사료', '수용비', '교재비', '재료비', '합계', '강사료', '수용비', '교재비', '재료비', '합계', '강사료', '수용비', '교재비', '재료비'])
ws.merge_cells('C1:G1'); ws.merge_cells('H1:L1'); ws.merge_cells('M1:Q1'); ws.merge_cells('R1:U1'); ws.merge_cells('V1:V2')
ws.append([1, '코딩', 70000, 3000, 28000, 0, 101000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 70000, 3000, 28000, 0])
ws = wb.create_sheet('부서별')
ws.append([None] * 6 + ['징수', None, None, None, '환불', None, None, None, '추가징수', None, None, None, '합계', None, None, None, '비고'])
ws.append(['순번', '부서', '학년', '반', '번호', '이름', '강사료', '수용비', '교재비', '재료비', '강사료', '수용비', '교재비', '재료비', '강사료', '수용비', '교재비', '재료비', '강사료', '수용비', '교재비', '재료비'])
ws.merge_cells('G1:J1'); ws.merge_cells('K1:N1'); ws.merge_cells('O1:R1'); ws.merge_cells('S1:V1')
data = [
    (1, '코딩A', *STUDENTS[0], 35000, 1500, 14000, 0, 0, 0, 0, 0, 0, 0, 0, 0),
    (2, '코딩A', *STUDENTS[1], 35000, 1500, 14000, 0, 0, 0, 0, 0, 0, 0, 0, 0),
    (3, '로봇과학A', *STUDENTS[3], 42000, 1800, 9000, 51000, 0, 0, 13500, 76500, 0, 0, 0, 0),
    (4, '컴퓨터C', *STUDENTS[6], 31500, 1350, 18000, 0, 0, 0, 0, 0, 31500, 1350, 18000, 0),
]
for d in data:
    n, dept, g, c, no, name, *amts = d
    col, ref, ext = amts[0:4], amts[4:8], amts[8:12]
    tot = [col[i] - ref[i] + ext[i] for i in range(4)]
    ws.append([n, dept, g, c, no, name, *col, *ref, *ext, *tot])
ws = wb.create_sheet('학생별')
ws.append(['학년', '반', '번호', '이름'])
save(wb, 'settle_s1.xlsm')

# ── A. 1학기 수강생 명단 ───────────────────────────────────────────────────
wb = Workbook(); ws = wb.active; ws.title = '학생명단'
ws.append([])
ws.append([None, '부서', '학년', '반', '이름', '비고'])
for dept, s in [('코딩A', STUDENTS[0]), ('코딩A', STUDENTS[1]), ('로봇과학A', STUDENTS[3]), ('바둑B', STUDENTS[7])]:
    ws.append([None, dept, s[0], s[1], s[3], None])
save(wb, 'roster_s1.xlsx')
print('done')
