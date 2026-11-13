// What `demi.transcript.watch` reads in the page (browse.md § The
// transcript's ruler): in every frame the page paints, where each row of a
// transcript stands in its view, and each thing the reader does to it, with
// when.
//
// `probe` runs in the page, from its source, so it uses nothing of this
// module but itself. It reads the page as drawn: no state of the list's own,
// so what it says holds for any list that carries the hooks, however the
// list is built.
//
// A frame is read as it is painted, by the last script the frame runs:
//
// ```text
//   animation-frame callbacks      the probe marks the frame
//   style and layout
//   resize observers, in rounds    the list measures its rows and lays
//                                  them out; the probe reads in the last round
//   paint
// ```
//
// A read in a task after the paint would also see what tasks before it
// changed, such as a key the page handled meanwhile, which that frame never
// showed. So the probe keeps a chain of hidden elements, each inside the one
// before, and observes their sizes: in each round it resizes the next, which
// the browser reports in the next round, and it reads in the last.

/** A row as a frame showed it: its key, how far its top stands below the view's top, and its height. */
export type RowReading = [key: string, top: number, height: number]

/**
 * A block of a row's text in view, such as a paragraph or a list item: an id
 * that names the same block from frame to frame, how far its top stands
 * below the view's top, its height, and the index of its row among the
 * frame's rows.
 */
export type BlockReading = [id: number, top: number, height: number, row: number]

/** What one painted frame showed. */
export interface Frame {
  /** When the frame began, in milliseconds on the page's clock. */
  t: number
  /** The scroller's `scrollTop`, `scrollHeight`, `clientHeight` and `clientWidth`; all 0 while the page shows no list. */
  st: number
  sh: number
  ch: number
  cw: number
  /** The top and the bottom of the box the rows stand in, below the view's top. */
  boxTop: number
  boxBottom: number
  /** Whether the list's Scroll to Bottom control is shown. */
  scrollToBottom: boolean
  rows: RowReading[]
  /** The blocks in view, from its top down. */
  blocks: BlockReading[]
  /**
   * How far the control the reader last opened or folded something with
   * stands from where it stood when they did, for the watch's `actedMs`
   * after; null once the page no longer holds it, absent otherwise.
   */
  acted?: number | null
  /** How far its row's top stands from where it stood then, and how much taller the row is; absent once the page no longer holds the row. */
  actedRow?: [top: number, taller: number]
  /**
   * Which list the page shows, by its hook's value (`data-transcript`), in
   * the first frame and in each frame it changed in; null while it shows none.
   */
  list?: string | null
}

/** What the reader did, and when the event happened, which is before the page handled it. */
export interface Input {
  t: number
  kind:
    /** A wheel's or a trackpad's turn over the list, `dy` down it; only its sign is relied on. */
    | 'wheel'
    /** A finger down on the list, moving, and lifted. */
    | 'touchstart' | 'touchmove' | 'touchend'
    /** A mouse or a pen held on the list or its scrollbar, and let go. */
    | 'hold' | 'release'
    /** A key that scrolls, pressed outside a field. */
    | 'key'
    /** A click, or Enter or Space, on something in a row. */
    | 'act'
    /** The list's Scroll to Bottom control. */
    | 'scroll-to-bottom'
    /** A message sent from the composer. */
    | 'send'
    /** The page went to the background, or came back. */
    | 'hidden' | 'shown'
  dy?: number
  /** The key, `Shift+Space` for Space with Shift, which scrolls the other way. */
  key?: string
  /** The key of the row acted on. */
  row?: string | null
  /** Whether what was acted on opens or folds something: a control that says so (`aria-expanded`), or a `<summary>`. */
  discloses?: boolean
  /**
   * The finger lifted from an element the page had removed meanwhile, as a
   * row that streams draws its text anew: such an event reaches that element
   * alone, and no listener on the window or the document hears it.
   */
  unheard?: boolean
}

/** What a watch read. */
export interface Recording {
  frames: Frame[]
  inputs: Input[]
}

/** What the page keeps of a watch, under `WATCH`. */
export interface Watching extends Recording {
  /** The element that scrolls the list's rows, once the page shows the list. */
  scroller(): HTMLElement | null
  /** Ends the watch and answers what it read. */
  stop(): Recording
}

/** The name the page keeps the watch under. */
export const WATCH = '__demiTranscriptWatch'

/**
 * Starts reading the `which`th transcript of the page, by `[data-transcript]`,
 * and keeps what it reads on the window as `name`, until its `stop` is called.
 * `actedMs` is how long a control the reader opened or folded something with
 * is followed.
 */
export function probe(which: number, name: string, actedMs: number): void {
  const TEXT = 'p, li, pre, table, h1, h2, h3, h4, h5, h6, blockquote, img, hr'
  /** The keys a browser scrolls a focused scroller by; Space is handled apart, since it also presses a control. */
  const SCROLL_KEYS = ['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End']
  /**
   * How many rounds of the frame's resize observers pass before the read. A
   * list measures the rows that changed in the first round and the rows it
   * mounted for them in the second; six leaves room for a list that takes more.
   */
  const ROUNDS = 6
  const frames: Frame[] = []
  const inputs: Input[] = []
  const ids = new Map<string, number>()
  let scroller: HTMLElement | null = null
  let acted: { element: Element, top: number, row: HTMLElement | null, rowTop: number, rowHeight: number, until: number } | null = null
  /** The list shown in the frame read last; undefined before the first. */
  let shown: string | null | undefined
  let frame = 0
  /** When the frame being made began; null once it is read. */
  let unread: number | null = null
  /** The round of the frame's resize observers that comes next. */
  let round = 0
  const round1 = (value: number) => Math.round(value * 10) / 10
  const list = () => document.querySelectorAll<HTMLElement>('[data-transcript]')[which] ?? null
  const inList = (target: EventTarget | null) => {
    const root = list()
    return root !== null && target instanceof Node && root.contains(target)
  }
  /** What holds the list and its dock, where its Scroll to Bottom control and its composer stand. */
  const surface = (): ParentNode => list()?.closest('[data-session-surface]') ?? document
  /** The element of `root` that scrolls its rows, `root` itself where it does. */
  const scrollerOf = (root: HTMLElement): HTMLElement | null => {
    for (const element of [root, ...root.querySelectorAll<HTMLElement>('*')]) {
      const overflow = getComputedStyle(element).overflowY
      if (overflow === 'auto' || overflow === 'scroll') {
        return element
      }
    }
    return null
  }
  /**
   * Whether `element` stands in a scroll area of its row's own, such as a
   * command's output: the reader scrolls it inside the row, which moves its
   * blocks and not the row. Kept per element, since an element keeps its
   * ancestors.
   */
  const scrolled = new WeakMap<Element, boolean>()
  const inOwnScroller = (row: HTMLElement, element: Element): boolean => {
    let known = scrolled.get(element)
    if (known === undefined) {
      known = false
      for (let at = element.parentElement; at && at !== row; at = at.parentElement) {
        const overflow = getComputedStyle(at).overflowY
        if (overflow === 'auto' || overflow === 'scroll') {
          known = true
          break
        }
      }
      scrolled.set(element, known)
    }
    return known
  }
  /** The id of the block `element` of `row`: by its place in the row and how its text begins, since a row that streams draws its blocks anew. */
  const idOf = (row: HTMLElement, element: Element): number => {
    const path: number[] = []
    for (let at = element; at !== row && at.parentElement; at = at.parentElement) {
      path.unshift(Array.prototype.indexOf.call(at.parentElement.children, at))
    }
    const first = document.createTreeWalker(element, NodeFilter.SHOW_TEXT).nextNode()
    const key = `${row.dataset['rowKey']}|${path.join('.')}|${element.tagName}|${(first?.nodeValue ?? '').slice(0, 24)}`
    let id = ids.get(key)
    if (id === undefined) {
      id = ids.size + 1
      ids.set(key, id)
    }
    return id
  }

  const note = (t: number, input: Omit<Input, 't'>) => inputs.push({ t: round1(t), ...input })
  let touching = false
  let held = false
  const act = (event: Event) => {
    const target = event.target
    if (!(target instanceof Element)) {
      return
    }
    const control = target.closest('button, summary, a, [role="button"]') ?? target
    const discloses = control.matches('[aria-expanded], summary')
    if (discloses) {
      const row = target.closest<HTMLElement>('[data-row-key]')
      const stands = row?.getBoundingClientRect()
      acted = {
        element: control,
        top: control.getBoundingClientRect().top,
        row,
        rowTop: stands?.top ?? 0,
        rowHeight: stands?.height ?? 0,
        until: performance.now() + actedMs,
      }
    }
    note(event.timeStamp, { kind: 'act', row: target.closest<HTMLElement>('[data-row-key]')?.dataset['rowKey'] ?? null, discloses })
  }
  const lifted = (event: Event, unheard: boolean) => {
    if (touching && (event.type === 'touchcancel' || (event as TouchEvent).touches.length === 0)) {
      touching = false
      note(event.timeStamp, unheard ? { kind: 'touchend', unheard } : { kind: 'touchend' })
    }
  }
  /** The finger's lift as the element it touched hears it, after the window would have. */
  const liftedThere = (event: Event) => lifted(event, true)
  const released = (event: Event) => {
    if (held) {
      held = false
      note(event.timeStamp, { kind: 'release' })
    }
  }
  /**
   * The reader may have sent a message: they did when the composer held
   * something and holds nothing once the page has handled the event. Enter in
   * an empty composer, or in one whose menu takes it, sends nothing.
   */
  const maySend = (event: Event) => {
    const composer = surface().querySelector('[data-composer]')
    if (!composer || (composer.textContent ?? '').trim() === '') {
      return
    }
    const when = event.timeStamp
    setTimeout(() => {
      if (!composer.isConnected || (composer.textContent ?? '').trim() === '') {
        note(when, { kind: 'send' })
      }
    })
  }
  const listeners: [target: EventTarget, type: string, listener: (event: Event) => void][] = [
    [document, 'wheel', (event) => {
      if (inList(event.target)) {
        note(event.timeStamp, { kind: 'wheel', dy: (event as WheelEvent).deltaY })
      }
    }],
    [document, 'touchstart', (event) => {
      if (inList(event.target)) {
        touching = true
        note(event.timeStamp, { kind: 'touchstart' })
        event.target?.addEventListener('touchend', liftedThere, { once: true })
        event.target?.addEventListener('touchcancel', liftedThere, { once: true })
      }
    }],
    [document, 'touchmove', (event) => {
      if (touching) {
        note(event.timeStamp, { kind: 'touchmove' })
      }
    }],
    [window, 'touchend', (event) => lifted(event, false)],
    [window, 'touchcancel', (event) => lifted(event, false)],
    [document, 'pointerdown', (event) => {
      if ((event as PointerEvent).pointerType !== 'touch' && inList(event.target)) {
        held = true
        note(event.timeStamp, { kind: 'hold' })
      }
    }],
    [window, 'pointerup', released],
    // A drag the browser takes over, of a picture or of selected text, ends the hold without a pointerup.
    [window, 'pointercancel', released],
    [document, 'click', (event) => {
      const target = event.target instanceof Element ? event.target : null
      const control = target?.closest('[data-scroll-to-bottom]')
      if (control && surface().contains(control)) {
        note(event.timeStamp, { kind: 'scroll-to-bottom' })
      } else if (target?.closest('[data-composer-send]') && surface().contains(target)) {
        maySend(event)
      } else if (inList(target)) {
        act(event)
      }
    }],
    [document, 'keydown', (event) => {
      const key = event as KeyboardEvent
      const target = key.target instanceof Element ? key.target : null
      if (target?.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])')) {
        if (key.key === 'Enter' && !key.shiftKey && !key.isComposing && target.closest('[data-composer]')) {
          maySend(event)
        }
      } else if ((key.key === 'Enter' || key.key === ' ') && inList(target) && target?.closest('button, summary, a, [role="button"]')) {
        act(event)
      } else if (key.key === ' ') {
        note(event.timeStamp, { kind: 'key', key: key.shiftKey ? 'Shift+Space' : 'Space' })
      } else if (SCROLL_KEYS.includes(key.key)) {
        note(event.timeStamp, { kind: 'key', key: key.key })
      }
    }],
    [document, 'visibilitychange', (event) => note(event.timeStamp, { kind: document.hidden ? 'hidden' : 'shown' })],
  ]
  for (const [target, type, listener] of listeners) {
    target.addEventListener(type, listener, { capture: true, passive: true })
  }

  const read = (t: number) => {
    const root = list()
    if (root && (!scroller || !scroller.isConnected || !root.contains(scroller))) {
      scroller = scrollerOf(root)
    }
    const id = root && scroller ? root.dataset['transcript'] ?? '' : null
    const changed = id !== shown ? { list: id } : {}
    shown = id
    if (!root || !scroller) {
      frames.push({ t, st: 0, sh: 0, ch: 0, cw: 0, boxTop: 0, boxBottom: 0, scrollToBottom: false, rows: [], blocks: [], ...changed })
      return
    }
    const view = scroller.getBoundingClientRect()
    const rows: RowReading[] = []
    const blocks: BlockReading[] = []
    let box: Element | null = null
    for (const row of scroller.querySelectorAll<HTMLElement>('[data-row-key]')) {
      // A row of a list drawn inside a row is that list's.
      if (row.closest('[data-transcript]') !== root) {
        continue
      }
      const at = row.getBoundingClientRect()
      rows.push([row.dataset['rowKey'] ?? '', round1(at.top - view.top), round1(at.height)])
      box ??= row.parentElement
      // Every block in view: the reader's place is the first two frames
      // share, and a block anywhere in it may move against its row.
      if (at.bottom <= view.top || at.top >= view.bottom) {
        continue
      }
      const text = row.querySelectorAll(TEXT)
      for (const element of text.length > 0 ? text : [row]) {
        const stands = element.getBoundingClientRect()
        if (stands.height === 0 || stands.bottom <= view.top) {
          continue
        }
        if (stands.top >= view.bottom) {
          break
        }
        // What the row clips away, as a folded row's content, shows nowhere.
        if (stands.top >= at.bottom || stands.bottom <= at.top || inOwnScroller(row, element)) {
          continue
        }
        blocks.push([idOf(row, element), round1(stands.top - view.top), round1(stands.height), rows.length - 1])
      }
    }
    const stands = box?.getBoundingClientRect()
    const reading: Frame = {
      t,
      st: round1(scroller.scrollTop),
      sh: scroller.scrollHeight,
      ch: scroller.clientHeight,
      cw: scroller.clientWidth,
      boxTop: stands ? round1(stands.top - view.top) : 0,
      boxBottom: stands ? round1(stands.bottom - view.top) : 0,
      scrollToBottom: surface().querySelector('[data-scroll-to-bottom]') !== null,
      rows,
      blocks,
      ...changed,
    }
    if (acted) {
      reading.acted = acted.element.isConnected ? round1(acted.element.getBoundingClientRect().top - acted.top) : null
      if (acted.row?.isConnected) {
        const row = acted.row.getBoundingClientRect()
        reading.actedRow = [round1(row.top - acted.rowTop), round1(row.height - acted.rowHeight)]
      }
      if (performance.now() > acted.until) {
        acted = null
      }
    }
    frames.push(reading)
  }

  // The chain the rounds go down, in a shadow tree of its own so that nothing
  // the page observes of its document changes.
  const host = document.createElement('div')
  host.setAttribute('aria-hidden', 'true')
  host.style.cssText = 'position:fixed;left:-100px;top:-100px;width:1px;height:1px;visibility:hidden;pointer-events:none'
  const links: HTMLElement[] = []
  let inside: Node = host.attachShadow({ mode: 'closed' })
  for (let link = 0; link < ROUNDS; link += 1) {
    const element = document.createElement('div')
    element.style.cssText = 'width:1px;height:1px;overflow:hidden'
    inside.appendChild(element)
    links.push(element)
    inside = element
  }
  const resize = (link: HTMLElement) => {
    link.style.width = link.style.width === '2px' ? '1px' : '2px'
  }
  const rounds = new ResizeObserver((entries) => {
    // One round a call: the first report of a link just observed comes with every other's.
    if (unread === null || !entries.some((entry) => entry.target === links[round])) {
      return
    }
    round += 1
    if (round < links.length) {
      resize(links[round]!)
      return
    }
    const t = round1(unread)
    unread = null
    read(t)
  })
  for (const link of links) {
    rounds.observe(link)
  }
  const tick = (now: number) => {
    // The document has no root element while it is being made.
    if (!host.isConnected && document.documentElement) {
      document.documentElement.appendChild(host)
    }
    unread = now
    round = 0
    resize(links[0]!)
    frame = requestAnimationFrame(tick)
  }
  frame = requestAnimationFrame(tick)

  const watching: Watching = {
    frames,
    inputs,
    scroller: () => scroller,
    stop: () => {
      cancelAnimationFrame(frame)
      rounds.disconnect()
      host.remove()
      for (const [target, type, listener] of listeners) {
        target.removeEventListener(type, listener, { capture: true })
      }
      Reflect.deleteProperty(window, name)
      return { frames, inputs }
    },
  }
  Object.assign(window, { [name]: watching })
}
