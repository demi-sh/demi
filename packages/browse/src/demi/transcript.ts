// `demi.transcript` (browse.md § The transcript's ruler): reads a transcript
// in every frame the page paints, and judges what it read by the rules a
// transcript keeps.
//
// - `demi.transcript.watch()` starts reading the page's transcript; its
//   `stop()` answers the recording.
// - `demi.transcript.judge(recording)` answers the recording's violations.
// - `demi.transcript.run(...names)` plays the scenes (`transcript/scenes.ts`),
//   all of them without a name, each in a new conversation with the scripted
//   model, and prints each one's violations.
// - `demi.transcript.planted()` plays the scenes with a planted defect
//   (`transcript/planted.ts`) and prints whether each one's rule reported it.
//
// A run sets the screen, the network and the processor's pace each scene
// needs, and leaves the slot's as it found them.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { CDPSession, Page } from 'playwright'
import { NORMAL } from '../network'
import { readState } from '../state'
import { Failure, webBase, type Tool } from '../tool'
import { writer, type Writer } from '../transcript/content'
import { PLANTED } from '../transcript/planted'
import { BLOCKS, probe, WATCH, type Recording, type Watching } from '../transcript/probe'
import { ACTED_MS, judge, type Violation } from '../transcript/rules'
import { SCENES } from '../transcript/scenes'
import type { EmulateOptions } from './emulate'
import type { helpers } from './index'

/** The helpers a scene has: every one but `demi.transcript`. */
type Demi = ReturnType<typeof helpers>

/**
 * How long a wait of a watch may take unless the call says otherwise: a
 * scene's longest answer streams for about twelve seconds, so a wait this
 * long is a hang, which the limit only guards against.
 */
const WAIT_MS = 60_000

export interface Watch {
  /** Waits until the list shows a row. */
  shown(options?: { timeout?: number }): Promise<void>
  /** Waits until the list's rows are `px` taller together than when this was called. */
  grown(px: number, options?: { timeout?: number }): Promise<void>
  /**
   * Waits until the list has shown the same rows at the same places for `ms`
   * from now on, half a second unless given: what the reader did just before
   * shows a frame or two later, and is not stillness.
   */
  still(ms?: number, options?: { timeout?: number }): Promise<void>
  /** Ends the watch; answers what it read. */
  stop(): Promise<Recording>
}

/** What a scene gets. */
export interface Lab {
  page: Page
  cdp: CDPSession
  demi: Demi
  /** The scene's writer of invented text, the same in every run of it. */
  text: Writer
  /**
   * Starts reading the page's transcript, once it stands still: a list still
   * on its way to the end of the turn before would be taken for one the
   * reader reads above its end.
   */
  watch(): Promise<Watch>
  /**
   * Loads the page anew, as a person who opens the conversation's address
   * does, with a watch that reads from the page's first frame.
   */
  cold(): Promise<Watch>
  /** Puts the pointer over the transcript, a third of the way down it, where a wheel's turn scrolls it. */
  point(): Promise<void>
}

/** A scene: what a conversation shows and what its reader does meanwhile. */
export interface Scene {
  name: string
  /** What happens, in a sentence. */
  about: string
  /** Plays the scene from a new conversation's page, and answers what a watch read of it. */
  play(lab: Lab): Promise<Recording>
}

/** A scene as it went. */
export interface Played {
  name: string
  frames: number
  seconds: number
  /** Frames the page took over 50 ms to paint after the one before: a stall the reader sees. */
  stalls: number
  violations: Violation[]
  /** Where the recording was written. */
  recording: string
}

/**
 * The probe's start as a script of the page: the probe comes as its source,
 * since a function cannot cross into the page. A watch the page already
 * holds ends first, and a frame inside the page, which has no transcript of
 * its own, starts none.
 */
function probeScript(which: number): string {
  const name = JSON.stringify(WATCH)
  return `if (window.top === window) { window[${name}]?.stop(); (${probe.toString()})(${which}, ${name}, ${BLOCKS}, ${ACTED_MS}) }`
}

/** Loads `page` anew with the probe started before any script of the page runs, and answers its watch. */
async function watchCold(page: Page, cdp: CDPSession, which: number): Promise<Watch> {
  // The browser runs a session's scripts in new documents only while the session's Page domain is on.
  await cdp.send('Page.enable')
  const { identifier } = await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: probeScript(which) })
  try {
    await page.reload()
  } finally {
    // The script is this load's: a later one starts no watch.
    await cdp.send('Page.removeScriptToEvaluateOnNewDocument', { identifier })
  }
  return watching(page)
}

async function watch(page: Page, which: number): Promise<Watch> {
  await page.evaluate(probeScript(which))
  return watching(page)
}

/** Ends the watch `page` holds, if any. */
function endWatch(page: Page): Promise<void> {
  return page.evaluate((name) => {
    (Reflect.get(window, name) as Watching | undefined)?.stop()
  }, WATCH)
}

/** The watch the probe started in `page`. */
function watching(page: Page): Watch {
  const until = (what: string, test: string, argument: unknown, timeout = WAIT_MS) =>
    page.waitForFunction(`(${test})(${JSON.stringify([WATCH, argument])})`, undefined, { timeout }).then(() => undefined, (error: unknown) => {
      throw new Failure(`The transcript did not ${what} within ${timeout / 1000} s: ${error instanceof Error ? error.message.split('\n')[0] : String(error)}`)
    })
  const height = () => page.evaluate((name) => {
    const last = (Reflect.get(window, name) as Recording | undefined)?.frames.at(-1)
    return last ? last.boxBottom - last.boxTop : 0
  }, WATCH)
  return {
    shown: (options) => until('show a row', ((([name]: [string, unknown]) =>
      ((Reflect.get(window, name) as Recording | undefined)?.frames.at(-1)?.rows.length ?? 0) > 0)).toString(), null, options?.timeout),
    grown: async (px, options) => {
      const from = await height()
      await until(`grow by ${px} px`, ((([name, target]: [string, number]) => {
        const last = (Reflect.get(window, name) as Recording).frames.at(-1)
        return last !== undefined && last.boxBottom - last.boxTop >= target
      })).toString(), from + px, options?.timeout)
    },
    still: async (ms = 500, options) => {
      const from = await page.evaluate(() => performance.now())
      await until(`stand still for ${ms} ms`, ((([name, [span, since]]: [string, [number, number]]) => {
        const frames = (Reflect.get(window, name) as Recording).frames
        const last = frames.at(-1)
        if (!last) {
          return false
        }
        const shows = (frame: Recording['frames'][number]) => `${frame.st}:${frame.sh}:${frame.rows.map((row) => `${row[1]},${row[2]}`).join(';')}`
        const same = shows(last)
        for (let at = frames.length - 1; at >= 0 && frames[at]!.t >= since; at -= 1) {
          if (shows(frames[at]!) !== same) {
            return false
          }
          if (last.t - frames[at]!.t >= span) {
            return true
          }
        }
        return false
      })).toString(), [ms, from], options?.timeout)
    },
    stop: () => page.evaluate((name) => {
      const held = Reflect.get(window, name) as Watching | undefined
      if (!held) {
        throw new Error('The page holds no watch: it loaded anew since the watch began')
      }
      return held.stop()
    }, WATCH),
  }
}

/** The name's letters as a number, a scene's seed. */
function seedOf(name: string): number {
  let seed = 0
  for (const letter of name) {
    seed = (Math.imul(seed, 31) + letter.charCodeAt(0)) >>> 0
  }
  return seed
}

export function transcript(tool: Tool, others: Demi) {
  const start = async (which = 0) => watch(await tool.browser.page(), which)

  /** Puts the slot's screen and network back as they are now; answers the function that does. */
  function conditions(): () => Promise<void> {
    const { emulation = {}, net = NORMAL } = readState(tool.slot)
    const screen: EmulateOptions = {
      reset: true,
      viewport: emulation.width !== undefined && emulation.height !== undefined ? { width: emulation.width, height: emulation.height } : undefined,
      scale: emulation.scale,
      theme: emulation.theme,
      device: emulation.device,
      locale: emulation.locale,
      timeZone: emulation.timezone,
      platform: emulation.platform,
    }
    return async () => {
      await others.emulate(screen)
      await others.net.reset()
      await others.net.latency(net.latencyMs)
      await others.net.bandwidth(net.kbps)
      if (net.reach === 'offline') {
        await others.net.offline()
      } else if (net.reach === 'unreachable') {
        await others.net.unreachable()
      }
    }
  }

  /**
   * How many times the web app's development server has sent the page a
   * changed module. The page then makes its components anew, the list among
   * them, which places itself again: a scene played meanwhile shows a jump
   * that is the reload's, not the list's.
   */
  function reloads(): number {
    const log = readState(tool.slot).servers.web?.log
    return log && existsSync(log) ? readFileSync(log, 'utf8').match(/hmr update|page reload/g)?.length ?? 0 : 0
  }

  /**
   * Plays `scene`, and again while the development server reloaded a module
   * meanwhile: three times at most, since a server that reloads through
   * three plays is one whose files someone is changing, and the scene's
   * result then says so.
   */
  async function play(scene: Scene): Promise<Played> {
    for (let again = 0; ; again += 1) {
      const before = reloads()
      const played = await playOnce(scene)
      if (reloads() === before) {
        return played
      }
      if (again === 2) {
        tool.print(`scene     ${scene.name}  NOT TO BE TRUSTED: the development server reloaded a module in each of three plays`)
        return played
      }
      tool.print(`scene     ${scene.name}  played again: the development server reloaded a module meanwhile`)
    }
  }

  /** Plays `scene` from a new conversation, under the conditions every scene starts from. */
  async function playOnce(scene: Scene): Promise<Played> {
    const page = await tool.browser.page()
    const cdp = await tool.browser.cdp()
    await others.emulate({ reset: true, viewport: { width: 1280, height: 860 } })
    await others.net.reset()
    await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 })
    others.model.forget()
    await page.goto(`${webBase(tool.slot)}/chat`)
    const lab: Lab = {
      page,
      cdp,
      demi: others,
      text: writer(seedOf(scene.name)),
      watch: async () => {
        const settling = await start()
        await settling.still(300)
        await settling.stop()
        return start()
      },
      cold: () => watchCold(page, cdp, 0),
      point: async () => {
        const box = await page.locator('[data-transcript]').first().boundingBox()
        if (!box) {
          throw new Failure('The page shows no transcript to point at')
        }
        await page.mouse.move(box.x + box.width / 2, box.y + box.height / 3)
      },
    }
    let recording: Recording
    try {
      recording = await scene.play(lab)
    } finally {
      // What a scene that failed midway left: its watch, a hidden page, a slow
      // processor, a cut network.
      await endWatch(page).catch(() => undefined)
      await others.show().catch(() => undefined)
      await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 }).catch(() => undefined)
      await others.net.reset().catch(() => undefined)
      // Its turn ends before the next scene queues its answers, which the
      // turn's next request would take.
      await others.turn({ timeout: WAIT_MS }).catch(() => undefined)
    }
    const folder = join(tool.slot.folder, 'transcript')
    mkdirSync(folder, { recursive: true })
    const path = join(folder, `${scene.name}.json`)
    writeFileSync(path, JSON.stringify(recording))
    const frames = recording.frames
    return {
      name: scene.name,
      frames: frames.length,
      seconds: frames.length > 0 ? Math.round((frames.at(-1)!.t - frames[0]!.t) / 100) / 10 : 0,
      stalls: frames.filter((frame, at) => at > 0 && frame.t - frames[at - 1]!.t > 50).length,
      violations: judge(recording),
      recording: path,
    }
  }

  return {
    /** Starts reading the page's `which`th transcript, the first unless given. */
    watch: (options?: { which?: number }) => start(options?.which),
    judge,
    /** The scenes, each by its name and what happens in it. */
    scenes: () => SCENES.map((scene) => ({ name: scene.name, about: scene.about })),
    /**
     * Plays the scenes with a planted defect and prints whether the rule of
     * each reported it; answers whether every one did, and none reported the
     * scene with no defect.
     */
    planted: async (): Promise<boolean> => {
      const restore = conditions()
      let held = true
      try {
        for (const scene of PLANTED) {
          const result = await play(scene)
          const reported = [...new Set(result.violations.map((violation) => violation.rule))]
          const right = scene.expects === null ? reported.length === 0 : reported.includes(scene.expects)
          held &&= right
          tool.print(`planted   ${scene.name}  ${right ? 'as expected' : 'NOT AS EXPECTED'}: ${scene.expects ?? 'no rule'} expected, ${reported.join(', ') || 'no rule'} reported`)
        }
      } finally {
        await restore()
      }
      return held
    },
    /** Plays the scenes named, or all of them, and prints how each went; answers the same. */
    run: async (...names: string[]): Promise<Played[]> => {
      const unknown = names.filter((name) => !SCENES.some((scene) => scene.name === name))
      if (unknown.length > 0) {
        throw new Failure(`No scene is named ${unknown.join(', ')}. The scenes: ${SCENES.map((scene) => scene.name).join(', ')}`)
      }
      const restore = conditions()
      const played: Played[] = []
      try {
        for (const scene of SCENES.filter((scene) => names.length === 0 || names.includes(scene.name))) {
          const result = await play(scene).catch((error: unknown) => {
            tool.print(`scene     ${scene.name}  did not play: ${error instanceof Error ? error.message.split('\n')[0] : String(error)}`)
            return null
          })
          if (!result) {
            continue
          }
          played.push(result)
          tool.print(`scene     ${result.name}  ${result.seconds} s, ${result.frames} frames, ${result.stalls} stalls, ${result.violations.length} violations`)
          for (const violation of result.violations) {
            tool.print(`            ${violation.rule}  at ${(violation.from / 1000).toFixed(2)} s, ${violation.frames} ${violation.frames === 1 ? 'frame' : 'frames'}: ${violation.what}`)
          }
        }
      } finally {
        await restore()
      }
      return played
    },
  }
}
