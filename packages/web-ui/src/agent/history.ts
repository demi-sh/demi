import { PAGE_BLOCKS } from '@jush-ai/protocol'
import {
  boundedTranscript,
  latestWindow,
  windowEnd,
  windowOf,
  type Held,
  type HeldBlock,
  type HeldTranscript,
  type TranscriptWindow,
} from '@jush-ai/conversation-client'

export type { HeldBlock } from '@jush-ai/conversation-client'

/**
 * The blocks at the transcript's end, its latest window, which the stream
 * keeps live: what a running turn, the message offered to edit and the tail
 * rows read.
 */
export function latestBlocks(history: HeldTranscript): HeldBlock[] {
  return latestWindow(history)?.blocks ?? []
}

/**
 * Rows that are no news to a reader: what the session writes about its own
 * upkeep, a compaction or a resume, and what no row shows.
 */
const NOT_NEWS = new Set(['compaction_boundary', 'compaction_marker', 'resume', 'context', 'response'])

/**
 * Where the transcript ends for a reader: its last block that is news, by
 * place, id and length of text, which grows as it streams; undefined without
 * the latest window.
 */
export function transcriptEnd(history: HeldTranscript): string | undefined {
  const window = latestWindow(history)
  if (!window)
    return undefined
  const index = window.blocks.findLastIndex((block) => !NOT_NEWS.has(block.type))
  const block = window.blocks[index]
  if (!block)
    return undefined
  const text = 'text' in block && typeof block.text === 'string' ? block.text.length : 0
  return `${window.start + index}:${block.id}:${text}`
}

/**
 * A block to bring into view and mark for a moment, in the transcript of the
 * agent `node`, the root's for null.
 */
export interface TranscriptReveal {
  node: string | null
  blockId: string
}

/**
 * How many blocks a transcript shown holds in memory on each side of the block
 * its view shows, a page before and a page after; what lies further leaves
 * memory and is read again, from the disk copy first, when the view reaches it.
 */
export const SHOWN_HALF = PAGE_BLOCKS

/**
 * What a view tells of where it stands in its transcript: the block in view,
 * or null for the latest, and how many blocks lie within its reach on either
 * side of that block.
 */
export interface ShownView {
  shownAt: string | null
  shownWithin?: number
}

/**
 * `history` bounded to what the view holds: `SHOWN_HALF` blocks beyond the
 * view's reach on each side of the block it shows, the latest, and the
 * indices `also` names, a page just read. The reach is what the view asks
 * pages for, so a transcript that folds into few rows holds as much as the
 * view needs to be satisfied, and never lets go of the page it just read.
 */
export function boundedHistory<B extends Held>(
  history: HeldTranscript<B>,
  view: ShownView,
  also?: { start: number; end: number },
): HeldTranscript<B> {
  return boundedTranscript(history, view.shownAt, SHOWN_HALF + (view.shownWithin ?? 0), also)
}

/**
 * The window the transcript shows: the one that holds `shownAt`, the block
 * the reader went to, or else the latest; none held yet, an empty one at
 * the start.
 */
export function shownWindow<B extends Held>(history: HeldTranscript<B>, shownAt: string | null): TranscriptWindow<B> {
  return (shownAt === null ? undefined : windowOf(history, shownAt))
    ?? latestWindow(history)
    ?? history.windows.at(-1)
    ?? { start: 0, blocks: [] }
}

/**
 * Where a window stands in its transcript: whether it reaches the start, the
 * end, and is the latest.
 */
export function windowEdges<B extends Held>(
  history: HeldTranscript<B>,
  window: TranscriptWindow<B>,
): { atStart: boolean; atEnd: boolean } {
  return { atStart: window.start === 0, atEnd: windowEnd(window) >= history.length }
}

/**
 * Where the page next to `window` on `side` is read from: the index of the
 * window's item at that edge and its id, which the page must still hold there;
 * none where the window reaches the transcript's start or end. /
 */
export function pageBeside<B extends Held>(
  history: HeldTranscript<B>,
  window: TranscriptWindow<B>,
  side: 'before' | 'after',
): { index: number; edge: string } | null {
  const edge = side === 'before' ? window.blocks[0] : window.blocks.at(-1)
  const { atStart, atEnd } = windowEdges(history, window)
  if (!edge || (side === 'before' ? atStart : atEnd)) {
    return null
  }
  return { index: side === 'before' ? window.start : windowEnd(window) - 1, edge: edge.id }
}

/**
 * A whole transcript held at once, as a gallery specimen or a test gives it.
 */
export function wholeHistory(blocks: HeldBlock[]): HeldTranscript {
  return { length: blocks.length, windows: [{ start: 0, blocks }] }
}
