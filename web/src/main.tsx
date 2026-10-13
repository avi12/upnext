import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { WindowFrame } from './components/WindowFrame'
import { markFrame } from './lib/frame'
import { reloadOnStaleScripts } from './lib/reload'
import { ErrorBoundary } from './components/ErrorBoundary'

markFrame()
reloadOnStaleScripts()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
    <WindowFrame />
  </StrictMode>,
)
