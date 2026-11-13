/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** The local development account, from the repository's `.env`; development builds only. */
  readonly JUSH_DEV_EMAIL?: string
  readonly JUSH_DEV_PASSWORD?: string
  /** This page's build, which `vite build` writes in (`vite.config.ts`); none in development. */
  readonly JUSH_WEB_BUILD?: string
  /** The Jush version this page belongs to (`vite.config.ts`). */
  readonly JUSH_VERSION: string
}

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<Record<string, unknown>, Record<string, unknown>, unknown>
  export default component
}
