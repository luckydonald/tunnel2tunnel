/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_SENTRY_DSN?: string
  readonly VITE_SENTRY_ENVIRONMENT?: string
  readonly VITE_SENTRY_RELEASE?: string
  readonly VITE_SENTRY_TRACES_SAMPLE_RATE?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

// Injected at build time via vite.config.ts's `define`.
declare const __GIT_COMMIT_FULL__: string
declare const __GIT_BRANCH__: string
declare const __BUILD_TIME__: string
