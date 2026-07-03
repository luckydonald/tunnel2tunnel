import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { sentryVitePlugin } from '@sentry/vite-plugin'
import { execSync } from 'node:child_process'

function run(cmd: string): string | null {
  try {
    return execSync(cmd, { encoding: 'utf8' }).trim()
  } catch {
    return null
  }
}

const buildInfo = {
  commit: process.env.SOURCE_COMMIT || run('git rev-parse HEAD') || 'unknown',
  branch: process.env.GIT_BRANCH || run('git rev-parse --abbrev-ref HEAD') || 'unknown',
}
const buildTime = process.env.BUILD_TIME || new Date().toISOString()

export default defineConfig({
  plugins: [
    vue(),
    // gate on non-VITE_-prefixed build secrets so a build without them
    // (local dev, CI without credentials) still succeeds — just without upload
    process.env.BUILD_BUGSINK_URL && process.env.BUILD_BUGSINK_AUTH_TOKEN
      ? sentryVitePlugin({
          url: process.env.BUILD_BUGSINK_URL,
          authToken: process.env.BUILD_BUGSINK_AUTH_TOKEN,
          org: 'none', // Bugsink has no orgs; any non-empty string is accepted
          project: process.env.BUILD_BUGSINK_PROJECT_SLUG ?? 'tunnel2tunnel-frontend',
          release: { name: buildInfo.commit, create: false, finalize: false, inject: true },
          sourcemaps: { filesToDeleteAfterUpload: ['./dist/**/*.map'] },
        })
      : null,
  ],
  define: {
    __GIT_COMMIT_FULL__: JSON.stringify(buildInfo.commit),
    __GIT_BRANCH__: JSON.stringify(buildInfo.branch),
    __BUILD_TIME__: JSON.stringify(buildTime),
  },
  resolve: {
    alias: {
      '@': new URL('./src', import.meta.url).pathname,
    },
  },
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:3000',
        changeOrigin: true,
      },
    },
  },
  build: {
    sourcemap: true,
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['vue', 'vue-router', 'pinia'],
        },
      },
    },
  },
})
