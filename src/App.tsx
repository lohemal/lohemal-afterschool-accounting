import { HashRouter, Route, Routes } from 'react-router-dom'

import { AppShell } from '@/components/AppShell'
import { Button, ToastProvider } from '@/components/ui'
import { errorMessage } from '@/ipc/api'
import { AppProvider } from '@/lib/useApp'
import { DashboardPage } from '@/pages/DashboardPage'
import { AdjustPage } from '@/pages/AdjustPage'
import { DiffPage } from '@/pages/DiffPage'
import { FundsPage } from '@/pages/FundsPage'
import { HistoryPage } from '@/pages/HistoryPage'
import { MonthlyPage } from '@/pages/MonthlyPage'
import { SettingsPage } from '@/pages/SettingsPage'
import { UploadPage } from '@/pages/UploadPage'

export default function App() {
  return (
    <ToastProvider>
      <AppProvider
        fallback={({ loading, error, retry }) => {
          if (loading) {
            return (
              <div className="center-box">
                <span className="spin" />
              </div>
            )
          }
          if (error) {
            return (
              <div className="center-box">
                <div className="card" style={{ maxWidth: 440 }}>
                  <div className="notice notice--bad">
                    <div>
                      <strong>자료를 불러오지 못했습니다.</strong>
                      <br />
                      {errorMessage(error)}
                    </div>
                  </div>
                  <div style={{ marginTop: 12 }}>
                    <Button variant="primary" onClick={retry}>
                      다시 시도
                    </Button>
                  </div>
                </div>
              </div>
            )
          }
          return null
        }}
      >
        <HashRouter>
          <Routes>
            <Route element={<AppShell />}>
              <Route index element={<DashboardPage />} />
              <Route path="/monthly" element={<MonthlyPage />} />
              <Route path="/upload" element={<UploadPage />} />
              <Route path="/funds" element={<FundsPage />} />
              <Route path="/diff" element={<DiffPage />} />
              <Route path="/adjust" element={<AdjustPage />} />
              <Route path="/history" element={<HistoryPage />} />
              <Route path="/settings" element={<SettingsPage />} />
            </Route>
          </Routes>
        </HashRouter>
      </AppProvider>
    </ToastProvider>
  )
}
