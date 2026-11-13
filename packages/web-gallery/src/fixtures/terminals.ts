import { DEMO_BUN_TEST, DEMO_GIT_DIFF, DEMO_RG } from './terminal-output'
import { endedTerminal, type EndedCommandEntry, type EndedOutputText, type TerminalRecord } from '@jush-ai/web-ui/agent/terminals'
import { BASH_SCRIPT, steppedRunCall, LEFT_RUNNING_CALL, RUNNING_RUN_CALL, RUNNING_SHELL_DESCRIPTION, RUNNING_SHELL_SCRIPT, returnedShellTool, runningShellTool } from './blocks'
import { ago } from './time'

/**
 * The live command: the running call of the demo transcript started it, so
 * its output shows under that call, and it prints as it runs
 * (`useLiveGalleryCommand`).
 */
export const LIVE_TERMINAL_ID = 'term-watch'

/** The command the running run's newest call started, a minute and a half ago (`runningRunBlocks`). */
export function runningRunCommand(): TerminalRecord {
  return {
    id: 'term-merge',
    title: 'Review and merge the pricing page copy',
    script: 'git merge --no-ff fix/reset-wording && bun run test',
    phase: 'running',
    startedAt: ago(92_000),
    output: 'Merge made by the \'ort\' strategy.\nbun test v1.3.14\n',
    toolUseId: `${RUNNING_RUN_CALL}-use`,
  }
}

/** The command the stepped run's running call started, 14 seconds ago, so its time shows; none when no call runs (`steppedRunBlocks`). */
export function steppedRunCommands(stage: number): TerminalRecord[] {
  const call = steppedRunCall(stage)
  return call
    ? [{ id: `term-${call.id}`, title: call.description, script: call.script, phase: 'running', startedAt: ago(14_000), output: '', toolUseId: `${call.id}-use` }]
    : []
}

/** The command the ended turn left running, started 2m 16s ago (`leftRunningBlocks`). */
export function leftRunningCommand(): TerminalRecord {
  return {
    id: 'term-e2e',
    title: 'Run the end-to-end suite',
    script: 'bun run e2e',
    phase: 'running',
    startedAt: ago(136_000),
    output: 'Running 140 specs\n',
    toolUseId: `${LEFT_RUNNING_CALL}-use`,
  }
}

/** A development server the agent started an hour ago, which runs until stopped. */
export function previewService(): TerminalRecord {
  return {
    id: 'term-preview',
    title: 'Serve the pricing page preview',
    script: 'bun run preview --port 4173',
    phase: 'running',
    startedAt: ago(3_600_000),
    output: 'Local: http://localhost:4173/\n',
    service: true,
  }
}

/** The conversation of the ended commands' list, whose own commands name it as their agent. */
const ROOT = 'gallery-conversation'

/** Each ended command the conversation lists, with the output the backend answers for it. */
const ENDED: readonly { entry: EndedCommandEntry; output: EndedOutputText }[] = [
  {
    entry: { commandId: 'term-docs', node: ROOT, title: 'Build the documentation site', service: false, startedAt: ago(7_260_000), endedAt: ago(7_200_000), end: { kind: 'exited', exitCode: 0 } },
    output: { script: 'bun run docs:build', output: 'Building 48 pages\nBuilt 48 pages in 3.1s\n', chars: 41, notStored: null },
  },
  {
    entry: { commandId: 'term-flaky', node: ROOT, title: 'Run the login test 20 times', service: false, startedAt: ago(5_520_000), endedAt: ago(5_400_000), end: { kind: 'exited', exitCode: 1 } },
    output: { script: 'for run in $(seq 20); do bun test src/login.test.ts || exit 1; done', output: 'run 1 passed\nrun 2 passed\nrun 3 failed: expected 200, got 401\n', chars: 62, notStored: null },
  },
  {
    entry: { commandId: 'term-archive', node: ROOT, title: 'Print the release archive', service: false, startedAt: ago(3_660_000), endedAt: ago(3_600_000), end: { kind: 'exited', exitCode: 0 } },
    output: { script: 'tar -czf - dist', output: '', chars: 0, notStored: 'larger than the backend keeps' },
  },
  {
    entry: { commandId: 'term-previews', node: ROOT, title: 'Serve the component previews', service: true, startedAt: ago(10_800_000), endedAt: ago(9_000_000), end: { kind: 'stopped' } },
    output: { script: 'bun run previews --port 6006', output: 'Local: http://localhost:6006/\n', chars: 30, notStored: null },
  },
]

/**
 * The commands the conversation's calls left running that ended, as its
 * list names them: their output is still to be read (`readGalleryOutput`).
 */
export function galleryEndedTerminals(): TerminalRecord[] {
  return ENDED.map(({ entry }) => endedTerminal(entry, ROOT, undefined))
}

/**
 * Reads the output of the ended command `terminal`, as the page asks the
 * backend for it: it loads for a moment, then shows.
 */
export function readGalleryOutput(terminal: TerminalRecord): void {
  const ended = ENDED.find(({ entry }) => entry.commandId === terminal.id)
  if (!ended) {
    return
  }
  terminal.outputRead = { state: 'reading' }
  setTimeout(() => {
    Object.assign(terminal, endedTerminal(ended.entry, ROOT, ended.output))
    delete terminal.outputRead
  }, 900)
}

export function galleryTerminals(): TerminalRecord[] {
  return [
    {
      id: 'term-test',
      title: 'Run the auth tests',
      script: 'bun test src/auth.test.ts',
      phase: 'running',
      startedAt: ago(60_000),
      output: DEMO_BUN_TEST,
      toolUseId: returnedShellTool.toolUseId,
    },
    {
      id: 'term-rg',
      title: 'Find where the old cookie name is still used',
      // Longer than the terminal is wide: it wraps as a terminal wraps what was typed.
      script: "rg --line-number --color=always --glob '!**/*.snap' --glob '!**/fixtures/**' --max-columns 200 sid packages/web/src packages/web-ui/src packages/web-gallery/src",
      phase: 'running',
      startedAt: ago(45_000),
      output: DEMO_RG,
      // The search of the running child Find the cookie assertion: stopping
      // the child leaves it stopping.
      subagentId: 'ag-cookie',
    },
    {
      id: 'term-diff',
      title: 'Show the auth test changes',
      script: 'git diff --stat\ngit diff packages/web/src/auth.test.ts',
      phase: 'running',
      startedAt: ago(30_000),
      output: DEMO_GIT_DIFF,
    },
    {
      id: 'term-bash',
      title: 'Count the old cookie name and list the changed files',
      script: BASH_SCRIPT,
      phase: 'running',
      startedAt: ago(20_000),
      output: '4\nchanged: packages/web/src/auth.test.ts\nchanged: packages/web/src/cookie.ts\n',
    },
    previewService(),
    {
      id: LIVE_TERMINAL_ID,
      title: RUNNING_SHELL_DESCRIPTION,
      script: RUNNING_SHELL_SCRIPT,
      phase: 'running',
      startedAt: ago(20_000),
      output: '',
      chars: 0,
      toolUseId: runningShellTool.toolUseId,
    },
  ]
}
