import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  define: {
    __GIT_COMMIT_FULL__: JSON.stringify('test'),
    __GIT_BRANCH__: JSON.stringify('test'),
    __BUILD_TIME__: JSON.stringify('test'),
  },
  resolve: {
    alias: {
      '@': new URL('./src', import.meta.url).pathname,
    },
  },
  test: {
    environment: 'jsdom',
    globals: false,
  },
})
