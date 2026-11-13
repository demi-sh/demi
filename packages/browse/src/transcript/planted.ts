// The defects `demi.transcript.planted` plants (browse.md § The transcript's
// ruler): each is a scene in which the script itself moves the list as a
// defect of the list would, and the rule that must report it. A ruler that
// reports nothing proves nothing until it is seen to report these.
import type { Page } from 'playwright'
import type { Lab, Scene } from '../demi/transcript'
import { WATCH, type Watching } from './probe'
import type { Rule } from './rules'
import { answered, wheel } from './scenes'

/** A scene with a planted defect, and the rule that reports it; null for a scene with none, which no rule may report. */
export type Planted = Scene & { expects: Rule | null }

/**
 * Scrolls the transcript by `by` pixels from the script, which no reader's
 * input explains, `frames` frames from now: in a frame of its own, after the
 * one that shows what the reader just did.
 */
function push(page: Page, by: number, frames = 0): Promise<void> {
  return page.evaluate(([by, frames, name]) => new Promise<void>((done) => {
    // The scroller the scene's watch reads.
    const scroller = (Reflect.get(window, name) as Watching | undefined)?.scroller()
    const after = (left: number) => {
      if (left > 0) {
        requestAnimationFrame(() => after(left - 1))
        return
      }
      if (scroller) {
        scroller.scrollTop += by
      }
      done()
    }
    after(frames)
  }), [by, frames, WATCH] as const)
}

/** Keeps the transcript `above` pixels above its end for `frames` frames, as a list that does not follow its end stays behind. */
function keepBehind(page: Page, above: number, frames: number): Promise<void> {
  return page.evaluate(([above, frames, name]) => new Promise<void>((done) => {
    const scroller = (Reflect.get(window, name) as Watching | undefined)?.scroller()
    const hold = (left: number) => {
      if (!scroller || left === 0) {
        done()
        return
      }
      scroller.scrollTop = scroller.scrollHeight - scroller.clientHeight - above
      requestAnimationFrame(() => hold(left - 1))
    }
    hold(frames)
  }), [above, frames, WATCH] as const)
}

/**
 * Slides the row or block nearest the middle of the window, from
 * `from` pixels above its place back to it over 200 ms, as a slide of the
 * list's own that runs on under the reader's scroll would.
 */
function slide(page: Page, pick: 'row' | 'block', from: number): Promise<void> {
  return page.evaluate(([pick, from]) => {
    const middle = innerHeight / 2
    const rows = [...document.querySelectorAll<HTMLElement>('[data-transcript] [data-row-key]')]
    const candidates = pick === 'row' ? rows : rows.flatMap((row) => [...row.querySelectorAll<HTMLElement>('p, li, pre, table')])
    const off = (candidate: HTMLElement) => {
      const stands = candidate.getBoundingClientRect()
      return Math.abs((stands.top + stands.bottom) / 2 - middle)
    }
    const target = candidates.sort((a, b) => off(a) - off(b))[0]
    target?.animate([{ transform: `translateY(${from}px)` }, { transform: 'translateY(0px)' }], { duration: 200, composite: 'add' })
  }, [pick, from] as const)
}

/** A conversation with an answer several views tall, drawn at once. */
const seeded = (lab: Lab) => answered(lab, 30)

export const PLANTED: Planted[] = [
  {
    name: 'at-rest',
    about: 'Nothing is planted: the reader scrolls up and reads a list that stands still.',
    expects: null,
    play: async (lab) => {
      await seeded(lab)
      const seen = await lab.watch()
      await wheel(lab, -500)
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'pushed-up-while-following',
    about: 'The view is pushed 40 px up the list under a reader who watches the end of an answer that streams.',
    expects: 'moved-down',
    play: async (lab) => {
      await seeded(lab)
      const seen = await lab.watch()
      await lab.demi.model.answers([[{ say: lab.text.prose(30), ms: 6000 }]])
      await lab.demi.message(lab.text.sentence(), { wait: false })
      await seen.grown(400)
      await push(lab.page, -40)
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'pushed-down-while-reading',
    about: 'The view is pushed 30 px down the list under a reader who reads above its end.',
    expects: 'place-moved',
    play: async (lab) => {
      await seeded(lab)
      const seen = await lab.watch()
      await wheel(lab, -600)
      await seen.still(400)
      await push(lab.page, 30)
      await seen.still(400)
      return seen.stop()
    },
  },
  {
    name: 'thrown-to-end-while-reading',
    about: 'The view is thrown to the list\'s end in one frame under a reader who reads above it.',
    expects: 'place-moved',
    play: async (lab) => {
      await seeded(lab)
      const seen = await lab.watch()
      await wheel(lab, -600)
      await seen.still(400)
      await push(lab.page, 100_000)
      await seen.still(400)
      return seen.stop()
    },
  },
  {
    name: 'left-behind-while-following',
    about: 'The view is kept 500 px above the end for half a second under a reader who watches the end of an answer that streams.',
    expects: 'end-left',
    play: async (lab) => {
      await seeded(lab)
      const seen = await lab.watch()
      await lab.demi.model.answers([[{ say: lab.text.prose(30), ms: 6000 }]])
      await lab.demi.message(lab.text.sentence(), { wait: false })
      await seen.grown(400)
      // Thirty frames, half a second: longer than a glide takes to reach an end that far.
      await keepBehind(lab.page, 500, 30)
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'pushed-back-while-scrolling',
    about: 'The view is pushed 80 px back down the list right after the reader turned the wheel up.',
    expects: 'pulled-back',
    play: async (lab) => {
      await seeded(lab)
      const seen = await lab.watch()
      await wheel(lab, -300)
      await push(lab.page, 80, 3)
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'row-slid-while-scrolling',
    about: 'A row in the middle of the view slides 60 px down to its place right after the reader turned the wheel up.',
    expects: 'moved-apart',
    play: async (lab) => {
      for (let turn = 0; turn < 8; turn++) {
        await answered(lab, 2)
      }
      const seen = await lab.watch()
      await wheel(lab, -300)
      await slide(lab.page, 'row', -60)
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'block-slid-while-scrolling',
    about: 'A paragraph in the middle of the view slides 60 px inside its row right after the reader turned the wheel up.',
    expects: 'moved-apart',
    play: async (lab) => {
      await seeded(lab)
      const seen = await lab.watch()
      await wheel(lab, -300)
      await slide(lab.page, 'block', -60)
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'scrolled-inside-a-row',
    about: 'Nothing is planted: the reader opens a command\'s row and turns the wheel down its output, which scrolls inside the row.',
    expects: null,
    play: async (lab) => {
      await seeded(lab)
      // A short answer after the command, so the opened row stands in view at the end.
      await lab.demi.model.answers([[{ run: 'seq 1 200' }], [{ say: lab.text.sentence() }]])
      await lab.demi.message(lab.text.sentence())
      await lab.page.locator('[data-transcript] [data-row-key] button[aria-expanded]').last().click()
      const output = lab.page.locator('[data-transcript] [data-row-key] pre').last()
      // The list goes to its end once the row has opened, before the reader's wheel.
      const opening = await lab.watch()
      await opening.still(1000)
      await opening.stop()
      const seen = await lab.watch()
      // Playwright's hover would scroll the list to show the whole output first.
      const box = await output.boundingBox()
      if (!box) {
        throw new Error('The opened command shows no output')
      }
      await lab.page.mouse.move(box.x + box.width / 2, Math.min(box.y + 40, lab.page.viewportSize()!.height - 200))
      // Down the output, which stands at its top: the output scrolls, and the list does not.
      for (let turn = 0; turn < 6; turn += 1) {
        await lab.page.mouse.wheel(0, 80)
        await lab.page.waitForTimeout(40)
      }
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'moved-after-opening',
    about: 'The view is pushed 25 px right after the reader opened a command\'s row.',
    expects: 'control-moved',
    play: async (lab) => {
      await seeded(lab)
      await lab.demi.model.answers([[{ run: 'seq 1 30' }], [{ say: lab.text.prose(3) }]])
      await lab.demi.message(lab.text.sentence())
      const seen = await lab.watch()
      await wheel(lab, -200)
      await seen.still(400)
      await lab.page.locator('[data-transcript] [data-row-key] button[aria-expanded]').last().click()
      await push(lab.page, -25)
      await seen.still(800)
      return seen.stop()
    },
  },
  {
    name: 'row-hidden',
    about: 'A row in the middle of the view is hidden.',
    expects: 'blank',
    play: async (lab) => {
      await seeded(lab)
      const seen = await lab.watch()
      await seen.still(300)
      await lab.page.evaluate(() => {
        const rows = [...document.querySelectorAll<HTMLElement>('[data-transcript] [data-row-key]')]
        const middle = innerHeight / 2
        const row = rows.find((candidate) => {
          const stands = candidate.getBoundingClientRect()
          return stands.top < middle && stands.bottom > middle
        })
        if (row) {
          row.style.visibility = 'hidden'
          row.style.height = '0px'
          row.style.overflow = 'hidden'
        }
      })
      await seen.still(300)
      return seen.stop()
    },
  },
]
