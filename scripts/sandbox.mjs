// 연습용 자료 폴더로 앱을 띄운다 — 실사용 DB(%APPDATA%\kr.school.asaccounting)를 건드리지 않는다.
import { spawn } from 'node:child_process'
import { mkdirSync } from 'node:fs'
import { join } from 'node:path'

const dir = join(process.cwd(), '.sandbox')
mkdirSync(dir, { recursive: true })
console.log(`[sandbox] ASACCOUNTING_DATA_DIR=${dir}`)
const child = spawn('npm', ['run', 'app'], {
  stdio: 'inherit',
  shell: true,
  env: { ...process.env, ASACCOUNTING_DATA_DIR: dir },
})
child.on('exit', (code) => process.exit(code ?? 0))
