// The scenes `demi.transcript.run` plays (browse.md § The transcript's ruler): each
// is a conversation the scripted model writes and what its reader does
// meanwhile, in the running product. A scene says nothing of what the list
// should do: the rules judge every frame of it.
//
// A scene waits for what the list shows, such as its growth by so many
// pixels, never for time to pass, so the reader acts at the same point of
// the transcript however fast the machine is.
import type { Lab, Scene } from '../demi/transcript'
import type { Answer } from '../model'

/** A conversation whose first turn has ended: a message and an answer of `parts` parts, drawn at once. */
export async function answered(lab: Lab, parts: number): Promise<void> {
  await lab.demi.model.answers([[{ say: lab.text.answer(parts) }]])
  await lab.demi.message(lab.text.sentence())
}

/** Queues `answers` and sends a message that starts their turn, without waiting for it. */
async function ask(lab: Lab, answers: Answer[]): Promise<void> {
  await lab.demi.model.answers(answers)
  await lab.demi.message(lab.text.sentence(), { wait: false })
}

/** A turn of `steps` commands with thinking before each, then an answer of `parts` parts streamed over `ms`. */
function work(lab: Lab, steps: number, parts: number, ms: number): Answer[] {
  return [
    ...Array.from({ length: steps }, (_, step): Answer => [
      { think: lab.text.paragraph(2), ms: 400 },
      { run: `sleep 0.2; seq 1 ${8 + step * 4}` },
    ]),
    [{ say: lab.text.answer(parts), ms }],
  ]
}

/** Turns the wheel over the transcript by `dy` pixels, down the list for a positive one. */
export async function wheel(lab: Lab, dy: number): Promise<void> {
  await lab.point()
  await lab.page.mouse.wheel(0, dy)
}

/** An answer that arrives `lots` times, a few paragraphs at once each time, `gap` milliseconds apart. */
function bursts(lab: Lab, lots: number, gap: number): Answer {
  return Array.from({ length: lots }, () => [{ say: `${lab.text.prose(2 + lab.text.pick(3))}\n\n` }, { wait: gap }]).flat()
}

export const SCENES: Scene[] = [
  {
    name: 'follow-stream',
    about: 'A long answer streams under an earlier one while the reader watches its end.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ think: lab.text.paragraph(3), ms: 600 }, { say: lab.text.answer(18), ms: 8000 }]])
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'follow-steps',
    about: 'A turn runs eight commands with thinking before each, then answers, while the reader watches its end.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, work(lab, 8, 8, 3000))
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'follow-stream-slow',
    about: 'A long answer streams while the reader watches its end, on a machine six times slower.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await lab.cdp.send('Emulation.setCPUThrottlingRate', { rate: 6 })
      await ask(lab, [[{ say: lab.text.answer(18), ms: 8000 }]])
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'read-earlier',
    about: 'The reader scrolls up into the earlier answer while a new one streams below.',
    play: async (lab) => {
      await answered(lab, 14)
      const seen = await lab.watch()
      await ask(lab, [[{ think: lab.text.paragraph(3), ms: 600 }, { say: lab.text.answer(16), ms: 7000 }]])
      await seen.grown(200)
      await wheel(lab, -700)
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'read-streaming-reply',
    about: 'The reader scrolls up a little within the answer that streams, and reads it while it grows.',
    play: async (lab) => {
      await answered(lab, 8)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(20), ms: 9000 }]])
      await seen.grown(900)
      await wheel(lab, -300)
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'read-while-steps-fold',
    about: 'The reader reads the earlier answer while a turn below runs six commands and answers.',
    play: async (lab) => {
      await answered(lab, 14)
      const seen = await lab.watch()
      await ask(lab, work(lab, 6, 6, 3000))
      await seen.grown(60)
      await wheel(lab, -600)
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'small-scroll-up',
    about: 'The reader turns the wheel up by a few pixels three times while the answer streams, which keeps the end in view.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.prose(24), ms: 8000 }]])
      for (let turn = 0; turn < 3; turn += 1) {
        await seen.grown(250)
        await wheel(lab, -5)
      }
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'wheel-down-at-end-while-following',
    about: 'The reader turns the wheel down at the list\'s end, where it scrolls nothing, while an answer arrives in lots of six paragraphs.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [Array.from({ length: 8 }, () => [{ say: `${lab.text.prose(6)}\n\n` }, { wait: 500 }]).flat()])
      await seen.grown(300)
      // Between two lots, once the list has reached its end.
      await seen.still(100)
      await wheel(lab, 120)
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'arrow-up-while-following',
    about: 'The reader clicks in the list and presses the Up arrow once while the answer streams, then reads.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.prose(24), ms: 8000 }]])
      await seen.grown(300)
      // A click on the text gives the list the keys.
      await lab.point()
      await lab.page.mouse.down()
      await lab.page.mouse.up()
      // The key comes once the list glides again after the click, not in the moment it rests for it.
      await seen.grown(120)
      await lab.page.keyboard.press('ArrowUp')
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'scroll-up-and-back',
    about: 'The reader scrolls up while the answer streams, reads, and scrolls back down to its end.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(22), ms: 9000 }]])
      await seen.grown(400)
      await wheel(lab, -500)
      await seen.grown(400)
      await wheel(lab, 5000)
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'send-while-reading',
    about: 'The reader reads the earlier answer and sends a message from there.',
    play: async (lab) => {
      await answered(lab, 14)
      const seen = await lab.watch()
      await wheel(lab, -700)
      await seen.still(300)
      await ask(lab, [[{ think: lab.text.paragraph(2), ms: 500 }, { say: lab.text.answer(12), ms: 5000 }]])
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'open-row-while-following',
    about: 'The reader opens the command\'s row while the answer after it streams, and folds it again.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [
        [{ run: 'seq 1 40' }],
        [{ say: lab.text.prose(18), ms: 8000 }],
      ])
      await seen.grown(300)
      // The control that opens the command's row: the last in the list that opens or folds something.
      const row = lab.page.locator('[data-transcript] [data-row-key] button[aria-expanded]').last()
      await row.click()
      await seen.grown(200)
      await row.click()
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'click-text-while-following',
    about: 'The reader clicks on the earlier answer\'s text three times while a new answer streams.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(22), ms: 9000 }]])
      for (let click = 0; click < 3; click += 1) {
        await seen.grown(300)
        await lab.point()
        await lab.page.mouse.down()
        await lab.page.mouse.up()
      }
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'select-text-while-following',
    about: 'The reader drags over the text to select it while a new answer streams.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(22), ms: 9000 }]])
      await seen.grown(300)
      const box = (await lab.page.locator('[data-transcript]').first().boundingBox())!
      await lab.page.mouse.move(box.x + box.width / 2 - 100, box.y + box.height / 3)
      await lab.page.mouse.down()
      await lab.page.mouse.move(box.x + box.width / 2 + 100, box.y + box.height / 3 + 60, { steps: 8 })
      await seen.grown(200)
      await lab.page.mouse.up()
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'bursts-while-following',
    about: 'An answer arrives a few paragraphs at a time, each lot at once, while the reader watches its end.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [bursts(lab, 12, 350)])
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'bursts-while-reading',
    about: 'An answer arrives a few paragraphs at a time, each lot at once, while the reader reads the earlier answer.',
    play: async (lab) => {
      await answered(lab, 14)
      const seen = await lab.watch()
      await ask(lab, [bursts(lab, 12, 350)])
      await seen.grown(150)
      await wheel(lab, -600)
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'open-long-message',
    about: 'The reader scrolls up to a long message of theirs that the list folded, opens it, and folds it again from its end.',
    play: async (lab) => {
      await answered(lab, 8)
      await lab.demi.model.answers([[{ say: lab.text.answer(5) }]])
      await lab.demi.message(Array.from({ length: 70 }, () => lab.text.sentence()).join('\n'))
      const seen = await lab.watch()
      // The control of the reader's folded message, which opens it and folds it.
      const fold = lab.page.locator('[data-transcript] [data-row-key^="message:"] [aria-expanded]').last()
      const view = (await lab.page.locator('[data-transcript]').first().boundingBox())!
      const middle = view.y + view.height / 2
      await wheel(lab, (await fold.boundingBox())!.y - middle)
      await seen.still(300)
      await fold.click()
      await seen.still(300)
      await wheel(lab, (await fold.boundingBox())!.y - middle)
      await seen.still(300)
      await fold.click()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'cut-while-following',
    about: 'The page loses its connection while the answer streams, and catches up when it is back.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(20), ms: 8000 }]])
      await seen.grown(300)
      await lab.demi.net.cut('/api/connect')
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'cut-while-reading',
    about: 'The page loses its connection while the answer streams and the reader reads the earlier answer.',
    play: async (lab) => {
      await answered(lab, 14)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(20), ms: 8000 }]])
      await seen.grown(200)
      await wheel(lab, -600)
      await seen.grown(200)
      await lab.demi.net.cut('/api/connect')
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'second-message-while-following',
    about: 'The reader sends a second message while the answer to the first streams.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(14), ms: 6000 }]])
      await seen.grown(300)
      await ask(lab, [[{ say: lab.text.answer(8), ms: 3000 }]])
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'second-message-in-short-list',
    about: 'In a new conversation, shorter than the view, the reader sends a second message while the first answer streams.',
    play: async (lab) => {
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.prose(5), ms: 5000 }]])
      await seen.shown()
      await seen.grown(60)
      await ask(lab, [[{ say: lab.text.prose(2), ms: 1500 }]])
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'nudges-up-at-rest',
    about: 'While an answer pauses, the reader turns the wheel up by ten pixels four times, a third of a second apart; then the answer goes on.',
    play: async (lab) => {
      await answered(lab, 12)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.prose(4), ms: 1200 }, { wait: 4000 }, { say: lab.text.prose(10), ms: 3000 }]])
      await seen.grown(150)
      // The pause: the list stands at its end, at rest.
      await seen.still(600)
      for (let turn = 0; turn < 4; turn += 1) {
        await wheel(lab, -10)
        await seen.still(300)
      }
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'cold-open-at-end',
    about: 'A conversation of formulas, code and tables is opened by its address, and shows its end.',
    play: async (lab) => {
      await lab.demi.model.answers([[{ say: Array.from({ length: 8 }, () => `${lab.text.paragraph()}\n\n${lab.text.formula()}\n\n${lab.text.code()}\n\n${lab.text.table()}`).join('\n\n') }]])
      await lab.demi.message(lab.text.sentence())
      const seen = await lab.cold()
      await seen.shown()
      await seen.still(1000)
      return seen.stop()
    },
  },
  {
    name: 'scroll-long-conversation',
    about: 'A long conversation of short turns and turns of work is opened afresh, and the reader scrolls it to its start and back with a trackpad.',
    play: async (lab) => {
      for (let turn = 0; turn < 6; turn++) {
        await answered(lab, 1)
      }
      for (let turn = 0; turn < 4; turn++) {
        await lab.demi.model.answers(work(lab, 2 + lab.text.pick(4), 6 + lab.text.pick(8), 0))
        await lab.demi.message(lab.text.sentence())
      }
      // The page opens it as one it holds nothing of, and reads its latest
      // blocks first: few enough here that the earlier ones arrive as a page
      // of their own once the first show, as in a transcript of thousands.
      // The page's copies on disk go too, or it would open from them; the
      // next scene starts from a new conversation, which has none.
      const first = /\/transcript\?latest=true&blocks=/
      let cut = 0
      await lab.page.route(first, (route) => {
        cut += 1
        return route.continue({ url: route.request().url().replace(/blocks=\d+/, 'blocks=12') })
      })
      let seen
      try {
        await lab.cdp.send('Storage.clearDataForOrigin', { origin: new URL(lab.page.url()).origin, storageTypes: 'indexeddb' })
        seen = await lab.cold()
        await seen.shown()
      } finally {
        await lab.page.unroute(first)
      }
      if (cut === 0) {
        throw new Error('The page opened the conversation without reading its latest blocks, so its earlier blocks never came as a page of their own')
      }
      await seen.still(500)
      await lab.point()
      // A trackpad's turns: many small ones, a frame or two apart.
      for (const way of [-1, 1]) {
        for (let turn = 0; turn < 160; turn++) {
          await lab.page.mouse.wheel(0, way * 60)
          await lab.page.waitForTimeout(24)
        }
        await seen.still(400)
      }
      return seen.stop()
    },
  },
  {
    name: 'back-to-reading-place',
    about: 'The reader reads the earlier answer, opens another conversation from the sidebar and comes back.',
    play: async (lab) => {
      await answered(lab, 18)
      const seen = await lab.watch()
      await wheel(lab, -800)
      await seen.still(300)
      // The sidebar's rows, by their hooks: this conversation's and any other of its kind.
      const own = /\/chat\/([^/?#]+)/.exec(lab.page.url())![1]!
      const row = lab.page.locator(`[data-sidebar-id="${own}"]`).first()
      const kind = await row.getAttribute('data-sidebar-kind')
      await lab.page.locator(`[data-sidebar-kind="${kind}"]:not([data-sidebar-id="${own}"])`).first().click()
      await lab.page.waitForURL((url) => !url.pathname.endsWith(own))
      await row.click()
      await seen.shown()
      await seen.still(1000)
      return seen.stop()
    },
  },
  {
    name: 'phone-swipe-while-streaming',
    about: 'On a phone, the reader swipes up into the earlier answer with a finger while a new one streams, and flicks back down.',
    play: async (lab) => {
      await lab.demi.emulate({ device: 'iPhone 15' })
      // The page reads its system as it starts: the phone's list is the one a phone gets.
      await lab.page.reload()
      await answered(lab, 12)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(20), ms: 9000 }]])
      await seen.grown(300)
      const box = (await lab.page.locator('[data-transcript]').first().boundingBox())!
      const at = { x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2) }
      // A finger's drag down the screen, which scrolls up the list, then a faster one the other way.
      await lab.cdp.send('Input.synthesizeScrollGesture', { ...at, yDistance: 400, speed: 900, gestureSourceType: 'touch' })
      await seen.grown(300)
      await lab.cdp.send('Input.synthesizeScrollGesture', { ...at, yDistance: -2400, speed: 4000, gestureSourceType: 'touch' })
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'phone-touch-streaming-text',
    about: 'On a phone, the reader puts a finger on the text that streams, holds it while more text comes, and lifts it.',
    play: async (lab) => {
      await lab.demi.emulate({ device: 'iPhone 15' })
      await lab.page.reload()
      await answered(lab, 6)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.prose(40), ms: 12000 }]])
      await seen.grown(500)
      // The reply's last paragraph, where its text comes in. It is read in
      // one step in the page: the list draws it anew with each piece of
      // text, and a locator's element would be gone before it is measured.
      const text = await lab.page.evaluate(() => {
        const paragraphs = document.querySelectorAll('[data-transcript] [data-row-key] p')
        const box = paragraphs[paragraphs.length - 1]!.getBoundingClientRect()
        return { x: Math.round(box.left + 40), y: Math.round(box.top + 8) }
      })
      await lab.cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [text] })
      await seen.grown(120)
      await lab.cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] })
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'narrower-while-reading',
    about: 'The window gets narrower and wider again while the reader reads the earlier answer.',
    play: async (lab) => {
      await answered(lab, 18)
      const seen = await lab.watch()
      await wheel(lab, -800)
      await seen.still(300)
      await lab.demi.emulate({ viewport: { width: 980, height: 860 } })
      await seen.still(300)
      await lab.demi.emulate({ viewport: { width: 1280, height: 860 } })
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'narrower-while-following',
    about: 'The window gets narrower and wider again while the answer streams and the reader watches its end.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(22), ms: 9000 }]])
      await seen.grown(400)
      await lab.demi.emulate({ viewport: { width: 980, height: 860 } })
      await seen.grown(400)
      await lab.demi.emulate({ viewport: { width: 1280, height: 860 } })
      await lab.demi.turn()
      await seen.still()
      return seen.stop()
    },
  },
  {
    name: 'composer-grows-while-following',
    about: 'The reader writes six lines in the composer while the answer streams, so the dock grows over the list\'s end.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(22), ms: 9000 }]])
      await seen.grown(300)
      const composer = lab.page.getByRole('textbox').and(lab.page.locator('[data-composer]'))
      for (let line = 0; line < 6; line += 1) {
        await composer.pressSequentially(lab.text.sentence().slice(0, 20))
        await composer.press('Shift+Enter')
      }
      await lab.demi.turn()
      await seen.still()
      // The draft is the scene's own: the next conversation starts without it.
      await composer.fill('')
      return seen.stop()
    },
  },
  {
    name: 'hidden-while-following',
    about: 'The page goes to the background while the answer streams, and comes back after it ended.',
    play: async (lab) => {
      await answered(lab, 10)
      const seen = await lab.watch()
      await ask(lab, [[{ say: lab.text.answer(16), ms: 5000 }]])
      await seen.grown(300)
      await lab.demi.hide()
      await lab.demi.turn()
      await lab.demi.show()
      await seen.still()
      return seen.stop()
    },
  },
]
