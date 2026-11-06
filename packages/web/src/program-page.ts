// Where the views run: the web app's own page, which reaches its server at
// `/api` behind a Demi session and reads the server's whole product state,
// or a program's page, which shows them in Demi's custom elements
// (`elements/main.ts`). Those reach one core at the path where the program
// forwards the core's routes, read the core's part of the product state,
// and act for whoever the program let in, with no session of Demi's.

import { shallowRef } from 'vue'

/** The program's page the views run in. */
export interface ProgramPage {
  /** Where the program forwards the core's routes, such as `/demi/api`. */
  api: string
}

// A ref, so what a store computed from it before the elements started, such
// as whose the browser's data is, follows.
const shown = shallowRef<ProgramPage | null>(null)

/** Makes this page a program's, whose core the program forwards at `api`; once, before the views start. */
export function showInProgramPage(api: string): void {
  shown.value = { api: api.replace(/\/+$/, '') }
}

/** The program's page the views run in, or null in the web app's own. */
export function programPage(): ProgramPage | null {
  return shown.value
}
