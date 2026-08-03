import { defineConfig } from 'vitest/config'
import { resolve } from 'path'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '#desktop': resolve(__dirname, 'src/platform'),
      '#features': resolve(__dirname, 'src/features'),
      '#ui': resolve(__dirname, 'src/components/ui'),
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    environmentOptions: {
      jsdom: {
        url: 'http://localhost/',
      },
    },
    exclude: ['**/node_modules/**', '**/dist/**'],
    setupFiles: './src/test/setup.ts',
    css: true,
  },
})
