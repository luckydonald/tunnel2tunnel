import * as Sentry from '@sentry/vue'
import type { App as VueApp } from 'vue'
import type { Router } from 'vue-router'

function parseSampleRate(raw: string | undefined): number {
  const parsed = Number(raw)
  return Number.isFinite(parsed) ? parsed : 0
}

export function initSentry(app: VueApp, router: Router): void {
  const dsn = import.meta.env.VITE_SENTRY_DSN
  if (!dsn) return

  Sentry.init({
    app,
    dsn,
    environment: import.meta.env.VITE_SENTRY_ENVIRONMENT || undefined,
    release: import.meta.env.VITE_SENTRY_RELEASE || __GIT_COMMIT_FULL__,
    integrations: [
      Sentry.browserTracingIntegration({ router }),
      Sentry.vueIntegration({ app, tracingOptions: { trackComponents: true } }),
    ],
    sendDefaultPii: false,
    initialScope: {
      tags: {
        git_commit: __GIT_COMMIT_FULL__,
        git_branch: __GIT_BRANCH__,
        build_time: __BUILD_TIME__,
      },
    },
    tracesSampleRate: parseSampleRate(import.meta.env.VITE_SENTRY_TRACES_SAMPLE_RATE),
  })
}
