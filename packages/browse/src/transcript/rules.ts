// The rules a transcript keeps in every frame it paints (browse.md § The
// transcript's ruler), and `judge`, which reads a recording against them.
//
// A rule says what a reader must see, never how a list does it, so one
// recording format and one set of rules judge any list. What the reader is
// doing decides which rules apply to a frame:
//
// ```text
//   following the end   the end stays in view; no row moves down
//   reading             the block at the view's top stays where it is,
//                       and is there again when the reader comes back
//   scrolling           the rows go the way the reader scrolls, as one:
//                       no row or block moves against those around it
//   opened a row        the row's top stays where it was
//   folded a row        the control they folded it with stays where it was
//   always              no blank in the view; the Scroll to Bottom control
//                       shows while reading, and only then
// ```
//
// Whether the reader follows or reads is decided by what they did, never by
// asking the list: the list's own idea of it is one of the things judged.
// The one exception is a row the reader opened or folded, after which the
// list follows its end or not by where that left it, a moment later: there
// the control tells which, and the rules hold the list to it from then on.
import { AT_END_PX, BOTTOM_THRESHOLD_PX } from '@demicodes/web-ui/composables/scroll-bottom'
import { FOLD_MS } from '@demicodes/web-ui/ui/fold'
import type { Frame, Input, Recording } from './probe'

export type Rule = 'end-left' | 'moved-down' | 'place-moved' | 'pulled-back' | 'moved-apart' | 'control-moved' | 'blank' | 'scroll-to-bottom'

/** One breach of a rule, over the frames that showed it without a pause. */
export interface Violation {
  rule: Rule
  /** Its first and last frame, in milliseconds since the recording's first frame. */
  from: number
  to: number
  /** How many frames showed it. */
  frames: number
  /** The largest distance among them, in pixels. */
  px: number
  what: string
}

/** Where a reader counts as at the end: the product's to decide, and the rules' to hold it to. */
export interface Spec {
  /** The end is in view while it lies less than this many pixels below the view. */
  atEnd: number
  /** A scroll down from further away that stops within this many pixels of the end rejoins it. */
  rejoin: number
}

export const SPEC: Spec = { atEnd: AT_END_PX, rejoin: BOTTOM_THRESHOLD_PX }

/**
 * How long after the reader opened or folded a row what they acted on must
 * stand still: the fold, and the frame or two the click takes to start it.
 */
export const ACTED_MS = FOLD_MS + 100

/** A distance of a pixel or less is the rounding of a fractional layout, not a move. */
const MOVED_PX = 1
/**
 * How far below the view the end may lie while the list glides to it: about
 * six lines of text, since a streamed piece can add several lines in a frame.
 */
const LAG_PX = 160
/**
 * For how long the end may lie further below the view than that, in the time
 * the page painted frames in: a glide brings what arrived at once within
 * reach in a tenth of a second. A frame the page was late with shows all
 * that arrived meanwhile and left the glide no frame to answer in, so it
 * counts for two frames' time at most.
 */
const LAG_MS = 100
/** How long after the list last grew its end must be back in view: the glide's few frames, with room. */
const SETTLE_MS = 500
/** How long a wheel's turn or a key goes on scrolling: a browser animates either for up to a few frames. */
const SCROLL_MS = 250
/** How long a flick may go on scrolling after the finger lifts. */
const MOMENTUM_MS = 3000
/**
 * How long the list has to reach the end after the reader asked for it, by
 * the control or by sending, or let go of the list while they follow it.
 */
const REACH_MS = 1000
/** How long the Scroll to Bottom control may take to show or to go: its fade, with room. */
const CONTROL_MS = 500
/** A gap this tall in the view is a blank: more than a line of text, where rows stand 4 px apart. */
const BLANK_PX = 32
/**
 * A frame at 60 Hz: an event that happened this long after a frame began
 * may have been handled before the frame was made, and shows in it.
 */
const FRAME_MS = 17
/** Breaches of one rule closer than this are one violation. */
const MERGE_MS = 300
/** How long the list has to reach its end again after the view's size or what covers its end changed. */
const RESIZE_MS = 300
/**
 * How long after the view got taller, or what covers its end shorter, the
 * rows may still come down by as much: the list answers such a change in
 * the next frame or two.
 */
const ROOM_MS = 150
/**
 * The room around the rows changed by more than this: the scroller counts its
 * height in whole pixels and the rows in fractions, so the two differ by a
 * pixel or so from frame to frame with no change of the room.
 */
const ROOM_PX = 2

type Mode = 'follow' | 'read'

/** The violations of `recording`, in the order they began. */
export function judge(recording: Recording, spec: Spec = SPEC): Violation[] {
  const violations: Violation[] = []
  const open = new Map<Rule, Violation>()
  const frames = recording.frames
  const start = frames[0]?.t ?? 0
  const breach = (rule: Rule, frame: Frame, px: number, what: string) => {
    const at = Math.round(frame.t - start)
    const last = open.get(rule)
    if (last && at - last.to <= MERGE_MS) {
      last.to = at
      last.frames += 1
      if (px > last.px) {
        last.px = Math.round(px)
        last.what = what
      }
      return
    }
    const violation = { rule, from: at, to: at, frames: 1, px: Math.round(px), what }
    open.set(rule, violation)
    violations.push(violation)
  }

  const inputs = [...recording.inputs].sort((a, b) => a.t - b.t)
  let next = 0
  /** The list the page shows, and how the reader left each list they went away from. */
  let shown: string | null = null
  const left = new Map<string, { mode: Mode, frame: Frame }>()
  let mode: Mode | null = null
  /** When the mode was last decided, and until when the list may still be on its way to the end. */
  let decided = 0
  let reaching = 0
  /** The furthest the end has lain below the view since the reader stopped following it. */
  let far = 0
  let scrollUntil = -Infinity
  /** The way the reader's scroll goes: 1 down the list, -1 up, 0 unknown. */
  let way: -1 | 0 | 1 = 0
  /** The way the view last went under a finger or a held pointer, whose scroll has no way of its own. */
  let viewWay = 0
  /**
   * Where the view stood when the list last had it at its place under a
   * reader who follows the end: where the list itself last moved it to, or
   * the very end. Null while the reader reads.
   */
  let home: number | null = null
  /**
   * How far up the list the reader's scrolls have taken the view from there,
   * at the highest it stood during the scroll under way: several small
   * scrolls add up, until the list moves the view again. The view's own
   * move tells it, since a wheel's event counts its turn in other pixels
   * than the page's under an emulated screen.
   */
  let up = 0
  /** Whether the reader followed the end when their scroll began. */
  let followed = false
  /**
   * Whether the reader's scroll brought the end into view and has not taken
   * the view up since: they follow the end then, however much arrived below
   * it while their finger stayed on the list.
   */
  let reachedEnd = false
  let touching = false
  let holding = false
  let momentumUntil = -Infinity
  let actUntil = -Infinity
  /** Whether the row the reader acted on changed its height since. */
  let actChanged = false
  /** Until when the list decides whether it follows its end, after a row the reader opened or folded. */
  let settleUntil = -Infinity
  let resizedUntil = -Infinity
  /**
   * How far the rows of a list that follows its end may come down: by what
   * the view grew and the room at its end shrank, until `roomUntil`.
   */
  let roomFor = 0
  let roomUntil = -Infinity
  let grewAt = 0
  /** For how long the end has lain more than `LAG_PX` below the view of a reader who follows it, in painted time. */
  let lagging = 0
  let wasBusy = false
  let wasActing = false
  let wasSettling = false
  /** The frame before the reader's scroll began; held in an object, since the function that sets it is made per frame. */
  const scroll: { from: Frame | null } = { from: null }
  let before: Frame | null = null

  for (const frame of frames) {
    if (frame.list !== undefined && frame.list !== shown) {
      // Another list: what comes next starts anew.
      if (shown !== null && before && mode) {
        left.set(shown, { mode, frame: before })
      }
      shown = frame.list
      before = null
      mode = null
      scroll.from = null
      lagging = 0
      roomFor = 0
      actUntil = -Infinity
      settleUntil = -Infinity
      wasBusy = false
      wasActing = false
      wasSettling = false
    }
    if (shown === null) {
      continue
    }
    if (frame.rows.length === 0) {
      // The list the reader was shown draws no row: its place and what they do carry over to the frame that does.
      if (before) {
        breach('blank', frame, frame.ch, 'the list drew no row')
      }
      continue
    }
    const end = frame.sh - frame.st - frame.ch
    const setMode = (to: Mode) => {
      if (to === 'read' && mode !== 'read') {
        far = end
      }
      mode = to
      decided = frame.t
    }
    /** The reader's scroll begins with this input, unless one is under way. */
    const begin = (t: number) => {
      if (touching || holding || t <= scrollUntil || t <= momentumUntil) {
        return
      }
      scroll.from = before ?? frame
      followed = mode === 'follow'
      up = home === null ? 0 : Math.max(0, home - scroll.from.st)
      viewWay = 0
      reachedEnd = false
    }
    /** The reader asked for the end, in this frame. */
    let asked = false
    for (; next < inputs.length && inputs[next]!.t <= frame.t + FRAME_MS; next += 1) {
      const input: Input = inputs[next]!
      switch (input.kind) {
        case 'wheel':
          begin(input.t)
          scrollUntil = input.t + SCROLL_MS
          way = (input.dy ?? 0) > 0 ? 1 : (input.dy ?? 0) < 0 ? -1 : way
          break
        case 'key':
          begin(input.t)
          scrollUntil = input.t + SCROLL_MS
          way = input.key === 'ArrowDown' || input.key === 'PageDown' || input.key === 'End' || input.key === 'Space' ? 1 : -1
          break
        case 'touchstart':
          begin(input.t)
          touching = true
          way = 0
          break
        case 'touchend':
          touching = false
          // A finger that did not move the view gives it no momentum; the
          // view of a reader who follows the end moves by itself meanwhile.
          momentumUntil = !followed || up >= MOVED_PX ? input.t + MOMENTUM_MS : -Infinity
          break
        case 'hold':
          begin(input.t)
          holding = true
          way = 0
          break
        case 'release':
          holding = false
          break
        case 'act':
          // A click that opens or folds nothing leaves the list as it was.
          if (input.discloses) {
            actUntil = input.t + ACTED_MS
            actChanged = false
          }
          break
        case 'scroll-to-bottom':
        case 'send':
          setMode('follow')
          reaching = input.t + REACH_MS
          // The scroll they made before it is over: the list goes to its end.
          scrollUntil = -Infinity
          momentumUntil = -Infinity
          asked = true
          break
      }
    }
    // A flick goes on until the view stands still.
    if (frame.t <= momentumUntil && before && Math.abs(frame.st - before.st) < 0.5) {
      momentumUntil = -Infinity
    }
    const busy = touching || holding || frame.t <= scrollUntil || frame.t <= momentumUntil
    const acting = frame.t <= actUntil
    if (acting && frame.acted !== undefined && (!frame.actedRow || Math.abs(frame.actedRow[1]) > MOVED_PX)) {
      actChanged = true
    }
    if (wasActing && !acting && actChanged) {
      settleUntil = frame.t + CONTROL_MS
    }
    const settling = !acting && frame.t <= settleUntil
    if (frame.t > roomUntil) {
      roomFor = 0
    }
    if (before && (frame.ch !== before.ch || frame.cw !== before.cw || Math.abs(room(frame) - room(before)) > ROOM_PX)) {
      resizedUntil = frame.t + RESIZE_MS
      const gave = Math.max(0, frame.ch - before.ch) + Math.max(0, room(before) - room(frame))
      if (gave > 0) {
        roomFor += gave
        roomUntil = frame.t + ROOM_MS
      }
      // A list whose text wraps anew is laid out anew: its rows go where the new lines put them.
      if (frame.cw !== before.cw) {
        roomFor = Infinity
        roomUntil = frame.t + ROOM_MS
      }
    }
    if (before && frame.boxBottom - frame.boxTop > before.boxBottom - before.boxTop + MOVED_PX) {
      grewAt = frame.t
    }

    if (mode === null) {
      const found: Mode = end < spec.atEnd ? 'follow' : 'read'
      setMode(found)
      grewAt = frame.t
      const was = left.get(shown)
      left.delete(shown)
      if (was?.mode === 'read') {
        // A reader who read above the end and comes back finds what they read where they left it.
        const shift = anchorShift(was.frame, frame)
        if (shift === null) {
          breach('place-moved', frame, 0, 'what the reader read was not in view when they came back to the conversation')
        } else if (Math.abs(shift) > MOVED_PX) {
          breach('place-moved', frame, Math.abs(shift), `what the reader read stood ${px(shift)} from where they left it when they came back`)
        }
      } else if (was?.mode === 'follow' && found === 'read') {
        breach('end-left', frame, end, `the reader followed the end, and came back to the conversation ${Math.round(end)} px above it`)
      }
    }
    if (mode !== 'follow') {
      home = null
    } else if (home === null || end < 1 || (!busy && before && Math.abs(frame.st - before.st) >= 0.5)) {
      home = frame.st
    }
    if (busy && scroll.from) {
      up = Math.max(up, (home ?? scroll.from.st) - frame.st)
      if (before && Math.abs(frame.st - before.st) >= 0.5) {
        viewWay = Math.sign(frame.st - before.st)
      }
      if (end < spec.atEnd) {
        reachedEnd = true
      } else if (viewWay < 0) {
        reachedEnd = false
      }
    }
    if (wasBusy && !busy && !asked) {
      const went = way !== 0 ? way : viewWay
      if (reachedEnd) {
        setMode('follow')
      } else if (followed && up < spec.atEnd) {
        // A reader who followed the end and took the view up by less than
        // brings the end out of view follows it on: a tap, a held pointer,
        // a wheel's small turn.
      } else if (went > 0) {
        setMode(end < spec.atEnd || (end <= spec.rejoin && far > spec.rejoin) ? 'follow' : 'read')
      } else if (went < 0) {
        setMode(end < spec.atEnd ? 'follow' : 'read')
      }
      if (mode === 'follow') {
        // The list stood still under the reader's hand, and goes to its end from where that left it.
        reaching = frame.t + REACH_MS
      }
    }
    if (wasActing && !acting && !actChanged && mode === 'follow') {
      // The list held the view for the click, and goes to its end from where that left it.
      reaching = frame.t + REACH_MS
    }
    if (wasSettling && !settling && !busy) {
      setMode(frame.scrollToBottom ? 'read' : 'follow')
      if (mode === 'follow') {
        reaching = frame.t + REACH_MS
      }
    }

    if (before) {
      const shift = anchorShift(before, frame)
      if (busy) {
        lagging = 0
        // The rows go the way the reader scrolls: a wheel's turn up the list
        // brings the rows down. A reader who follows the end and turns the
        // wheel up by less than leaves it is followed on.
        const leaves = way > 0 || !followed || up >= spec.atEnd
        if (way !== 0 && leaves && shift !== null && shift * way > MOVED_PX) {
          breach('pulled-back', frame, Math.abs(shift), `the rows moved ${px(shift)} against the reader's scroll ${way > 0 ? 'down' : 'up'}`)
        }
        const torn = movedApart(before, frame)
        if (torn) {
          breach('moved-apart', frame, torn.by, torn.what)
        }
      } else if (acting) {
        lagging = 0
        const row = frame.actedRow
        if (row && row[1] < -MOVED_PX) {
          // A row folded from a control at its end gets shorter above the control, which stays under the pointer.
          if (typeof frame.acted === 'number' && Math.abs(frame.acted) > MOVED_PX) {
            breach('control-moved', frame, Math.abs(frame.acted), `the control the reader folded a row with moved ${px(frame.acted)}`)
          }
        } else if (row) {
          // A row that opens grows below its top: its control may go down with what it opened.
          if (Math.abs(row[0]) > MOVED_PX) {
            breach('control-moved', frame, Math.abs(row[0]), `the row the reader opened moved ${px(row[0])}`)
          }
        } else if (typeof frame.acted === 'number' && Math.abs(frame.acted) > MOVED_PX) {
          breach('control-moved', frame, Math.abs(frame.acted), `the control the reader clicked moved ${px(frame.acted)}`)
        }
      } else if (settling) {
        lagging = 0
      } else if (mode === 'follow') {
        if (frame.t > reaching) {
          const down = movedDown(before, frame)
          const shorter = (before.boxBottom - before.boxTop) - (frame.boxBottom - frame.boxTop)
          if (down && down.by <= roomFor + MOVED_PX) {
            roomFor -= down.by
          } else if (down && shorter <= MOVED_PX && end <= MOVED_PX) {
            // The view stands as far down as the scroller takes it, as in a
            // list no taller than the view: what grew took the rows after
            // it down, which no scroll could have kept still.
          } else if (down) {
            breach('moved-down', frame, down.by, shorter > MOVED_PX
              ? `the list got ${Math.round(shorter)} px shorter, and the row ${down.key} moved ${Math.round(down.by)} px down`
              : `the row ${down.key} moved ${Math.round(down.by)} px down while the list followed its end`)
          }
          lagging = end > LAG_PX ? lagging + Math.min(frame.t - before.t, 2 * FRAME_MS) : 0
          if (lagging > LAG_MS) {
            breach('end-left', frame, end, `the end lay ${Math.round(end)} px below the view while the list followed it`)
          } else if (end >= spec.atEnd && frame.t - grewAt > SETTLE_MS && frame.t > resizedUntil) {
            breach('end-left', frame, end, `the end stayed ${Math.round(end)} px below the view after the list stopped growing`)
          }
        } else {
          lagging = 0
        }
        if (frame.scrollToBottom && frame.t - decided > CONTROL_MS && frame.t > reaching) {
          breach('scroll-to-bottom', frame, 0, 'the Scroll to Bottom control showed while the list followed its end')
        }
      } else {
        lagging = 0
        far = Math.max(far, end)
        // A paragraph that wraps anew under the view's top keeps its share above it, not its top.
        if (shift !== null && Math.abs(shift) > MOVED_PX && frame.cw === before.cw) {
          breach('place-moved', frame, Math.abs(shift), `what the reader read moved ${px(shift)} with no scroll of theirs`)
        }
        if (!frame.scrollToBottom && end >= spec.atEnd && frame.t - decided > CONTROL_MS) {
          breach('scroll-to-bottom', frame, 0, 'the Scroll to Bottom control was gone while the reader read above the end')
        }
        if (end < spec.atEnd) {
          // The end came into view under a reader who did nothing, as when the view got taller.
          setMode('follow')
        }
      }
      const gap = blank(frame)
      if (gap) {
        breach('blank', frame, gap.px, `${Math.round(gap.px)} px of the view showed no row, from ${Math.round(gap.from)} px below its top`)
      }
    }
    wasBusy = busy
    wasActing = acting
    wasSettling = settling
    before = frame
  }
  return violations
}

/** The room around the rows inside the scroller: its padding, and what the dock adds over the list's end. */
function room(frame: Frame): number {
  return frame.sh - (frame.boxBottom - frame.boxTop)
}

/** A distance with its direction on the screen. */
function px(shift: number): string {
  return `${Math.abs(Math.round(shift * 10) / 10)} px ${shift > 0 ? 'down' : 'up'}`
}

/**
 * How far what stood at the view's top moved between two frames: the first
 * block from the top that both show, or the first row of the view; null when
 * they share nothing of what the first showed.
 */
function anchorShift(before: Frame, frame: Frame): number | null {
  const now = new Map(frame.blocks.map((block) => [block[0], block[1]]))
  for (const [id, top] of [...before.blocks].sort((a, b) => a[1] - b[1])) {
    const stands = now.get(id)
    if (stands !== undefined) {
      return stands - top
    }
  }
  const rows = new Map(frame.rows.map((row) => [row[0], row[1]]))
  for (const [key, top, height] of [...before.rows].sort((a, b) => a[1] - b[1])) {
    if (top + height <= 0 || top >= before.ch) {
      continue
    }
    const stands = rows.get(key)
    if (stands !== undefined) {
      return stands - top
    }
  }
  return null
}

/** The row in view that moved furthest down between two frames, by more than rounding. */
function movedDown(before: Frame, frame: Frame): { key: string, by: number } | null {
  const now = new Map(frame.rows.map((row) => [row[0], row[1]]))
  let worst: { key: string, by: number } | null = null
  for (const [key, top, height] of before.rows) {
    if (top + height <= 0 || top >= before.ch) {
      continue
    }
    const stands = now.get(key)
    if (stands !== undefined && stands - top > MOVED_PX && stands - top > (worst?.by ?? 0)) {
      worst = { key, by: stands - top }
    }
  }
  return worst
}

/**
 * The row or block in view that moved furthest against what stands around
 * it between two frames: a row against the row above it, which takes it
 * along by as much as it grew or shrank, or a block inside its row by more
 * than the row grew or shrank. A scroll moves them all as one, so a move of
 * one against the others is the list's own, such as a slide that runs on.
 */
function movedApart(before: Frame, frame: Frame): { by: number, what: string } | null {
  // A list whose text wraps anew is laid out anew.
  if (frame.cw !== before.cw) {
    return null
  }
  const was = new Map(before.rows.map((row) => [row[0], row]))
  const order = new Map([...before.rows].sort((a, b) => a[1] - b[1]).map((row, index) => [row[0], index]))
  const inView = (top: number, height: number) => top + height > 0 && top < frame.ch
  let worst: { by: number, what: string } | null = null
  const worse = (by: number, what: string) => {
    if (Math.abs(by) > MOVED_PX && Math.abs(by) > (worst?.by ?? 0)) {
      worst = { by: Math.abs(by), what }
    }
  }
  const rows = [...frame.rows].sort((a, b) => a[1] - b[1])
  for (let index = 1; index < rows.length; index += 1) {
    const [key, top, height] = rows[index]!
    const [aboveKey, aboveTop, aboveHeight] = rows[index - 1]!
    const then = was.get(key)
    const aboveThen = was.get(aboveKey)
    // A row that came or went between them leaves the two no measure of each other.
    if (!then || !aboveThen || !inView(top, height) || order.get(key) !== order.get(aboveKey)! + 1) {
      continue
    }
    const by = (top - then[1]) - (aboveTop - aboveThen[1]) - (aboveHeight - aboveThen[2])
    worse(by, `the row ${key} moved ${px(by)} against the row above it while the reader scrolled`)
  }
  const blocks = new Map(before.blocks.map((block) => [block[0], block]))
  for (const [id, top, height, index] of frame.blocks) {
    const then = blocks.get(id)
    const row = frame.rows[index]
    const rowThen = then && before.rows[then[3]]
    if (!then || !row || !rowThen || rowThen[0] !== row[0] || !inView(top, height)) {
      continue
    }
    const moved = (top - row[1]) - (then[1] - rowThen[1])
    const grew = row[2] - rowThen[2]
    const by = moved - Math.min(Math.max(moved, Math.min(0, grew)), Math.max(0, grew))
    worse(by, `a block of the row ${row[0]} moved ${px(by)} inside it while the reader scrolled`)
  }
  return worst
}

/** The tallest part of the view, of the part the rows' box covers, where no row stands. */
function blank(frame: Frame): { from: number, px: number } | null {
  const top = Math.max(0, frame.boxTop)
  const bottom = Math.min(frame.ch, frame.boxBottom)
  let covered = top
  let worst: { from: number, px: number } | null = null
  const gap = (until: number) => {
    if (until - covered > BLANK_PX && until - covered > (worst?.px ?? 0)) {
      worst = { from: covered, px: until - covered }
    }
  }
  for (const [, at, height] of [...frame.rows].sort((a, b) => a[1] - b[1])) {
    if (at + height <= covered) {
      continue
    }
    if (at >= bottom) {
      break
    }
    gap(at)
    covered = Math.max(covered, at + height)
  }
  gap(bottom)
  return worst
}
