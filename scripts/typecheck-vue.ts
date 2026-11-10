/**
 * Type-checks Vue projects, templates included:
 * `bun scripts/typecheck-vue.ts <tsconfig>...` prints every error and exits 2
 * when there is one. The program is the one vue-tsc would build
 * (`vue-program.ts` says why this does not run vue-tsc).
 *
 * A project checks only its own files: the ones its tsconfig includes, and
 * the JSON files in its directory it imports, which TypeScript leaves out of
 * the included files unless a pattern names `.json`. The files it imports
 * from another project, such as web-ui's in a plugin page, give it their
 * types but are checked by the project that owns them, so each file is
 * checked once, as a TypeScript project reference would have it. Checking
 * every file of every program checks web-ui again in each project that
 * imports it, several times the memory and the time. A project whose
 * imports must also be checked under its own options, such as a service
 * worker's against the worker's globals, is plain TypeScript and is checked
 * whole by tsgo instead.
 */
import { dirname, resolve } from 'node:path'
import ts from 'typescript'
import { createVueProgram } from './vue-program'

function check(project: string): readonly ts.Diagnostic[] {
  const created = createVueProgram(project)
  if (!created.ok)
    return created.errors
  const { program } = created
  const roots = new Set(program.getRootFileNames())
  const directory = `${dirname(project).replaceAll('\\', '/')}/`
  const own = (file: ts.SourceFile) =>
    roots.has(file.fileName) || (file.fileName.endsWith('.json') && file.fileName.startsWith(directory))
  const diagnostics = [...created.errors, ...program.getOptionsDiagnostics(), ...program.getGlobalDiagnostics()]
  for (const file of program.getSourceFiles()) {
    if (own(file))
      diagnostics.push(...program.getSyntacticDiagnostics(file), ...program.getSemanticDiagnostics(file))
  }
  return ts.sortAndDeduplicateDiagnostics(diagnostics)
}

const projects = process.argv.slice(2)
if (projects.length === 0) {
  console.error('Usage: bun scripts/typecheck-vue.ts <tsconfig>...')
  process.exit(1)
}
const host: ts.FormatDiagnosticsHost = {
  getCanonicalFileName: (fileName) => fileName,
  getCurrentDirectory: () => process.cwd(),
  getNewLine: () => ts.sys.newLine,
}
const format = process.stdout.isTTY ? ts.formatDiagnosticsWithColorAndContext : ts.formatDiagnostics
let errors = 0
for (const project of projects) {
  const diagnostics = check(resolve(project))
  errors += diagnostics.length
  if (diagnostics.length > 0)
    process.stdout.write(format(diagnostics, host))
}
if (errors > 0) {
  console.log(`Found ${errors} error${errors === 1 ? '' : 's'}.`)
  process.exit(2)
}
