import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './App.css'

function renderFatalError(error: unknown) {
  const root = document.getElementById('root')
  if (!root) return

  const message = error instanceof Error ? error.message : String(error)
  root.innerHTML = `
    <div style="height:100vh;display:flex;align-items:center;justify-content:center;background:#f8fafc;color:#1e293b;font-family:Segoe UI,Arial,sans-serif;padding:24px;">
      <div style="max-width:680px;width:100%;border:1px solid #e2e8f0;border-radius:8px;background:white;padding:20px;box-shadow:0 10px 30px rgba(15,23,42,.08);">
        <h1 style="font-size:18px;margin:0 0 10px;">PolyChat failed to start</h1>
        <p style="font-size:13px;line-height:1.6;margin:0;color:#64748b;">${message.replace(/[&<>"']/g, char => ({
          '&': '&amp;',
          '<': '&lt;',
          '>': '&gt;',
          '"': '&quot;',
          "'": '&#39;',
        })[char] ?? char)}</p>
      </div>
    </div>
  `
}

window.addEventListener('error', event => {
  renderFatalError(event.error ?? event.message)
})

window.addEventListener('unhandledrejection', event => {
  console.error('[polychat] unhandled promise rejection', event.reason)
})

try {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
} catch (error) {
  renderFatalError(error)
}
