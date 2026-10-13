/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { fileURLToPath, URL } from 'node:url'

// 같은 PC 의 다른 Tauri 프로젝트와 포트가 겹치지 않게 1470 을 쓴다
// (1420 통합 매니저 · 1421 학생명단 · 1430 QuickDeck · 1440 · 1450 경력증명서 · 1460).
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  clearScreen: false,
  server: {
    port: 1470,
    strictPort: true,
    // src-tauri/target 안의 실행 파일은 빌드 중 잠기므로 감시 대상에서 뺀다.
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: { target: 'chrome110', outDir: 'dist' },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
})
