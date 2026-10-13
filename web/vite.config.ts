import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  base: '/',
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      // The browser's Host goes through, so a write's Origin (this dev
      // server's port) matches the Host's port, as UpNext checks.
      '/api': {
        target: 'http://127.0.0.1:8787',
        changeOrigin: false,
      },
      // Settings › About's link; the server serves it, not the UI.
      '/third-party-notices.txt': 'http://127.0.0.1:8787',
    },
  },
  build: {
    outDir: 'dist',
  },
})
