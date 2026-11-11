// The invented text a scene's model answers with (browse.md § The
// transcript's ruler): Markdown of the shapes a transcript draws, from a
// seed, so a scene's transcript is the same in every run.

/** A source of numbers from 0 up to 1, the same for the same seed (mulberry32). */
export function numbers(seed: number): () => number {
  let state = seed >>> 0
  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let mixed = state
    mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1)
    mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61)
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296
  }
}

const WORDS = [
  'harbor', 'ledger', 'quiet', 'signal', 'branch', 'window', 'copper', 'engine', 'margin', 'lantern',
  'thread', 'summit', 'orchard', 'ripple', 'anchor', 'valley', 'ember', 'compass', 'meadow', 'cipher',
  'gather', 'steady', 'narrow', 'bright', 'carry', 'follow', 'measure', 'return', 'between', 'under',
  'the', 'a', 'of', 'and', 'with', 'each', 'before', 'after', 'while', 'until',
]

export type Writer = ReturnType<typeof writer>

/** A writer of invented Markdown from `seed`. */
export function writer(seed: number) {
  const random = numbers(seed)
  const pick = (count: number) => Math.floor(random() * count)
  const word = () => WORDS[pick(WORDS.length)]!
  const sentence = () => {
    const words = Array.from({ length: 6 + pick(12) }, word)
    return `${words[0]![0]!.toUpperCase()}${words[0]!.slice(1)} ${words.slice(1).join(' ')}.`
  }
  const paragraph = (sentences = 2 + pick(4)) => Array.from({ length: sentences }, sentence).join(' ')
  const items = (count = 3 + pick(4)) => Array.from({ length: count }, () => `- ${sentence()}`).join('\n')
  const code = (lines = 4 + pick(8)) =>
    `\`\`\`ts\n${Array.from({ length: lines }, (_, line) => `const ${word()}${line} = ${word()}(${pick(100)}) // ${word()} ${word()}`).join('\n')}\n\`\`\``
  const table = (rows = 3 + pick(3)) =>
    ['| Name | State | Note |', '| --- | --- | --- |', ...Array.from({ length: rows }, () => `| ${word()} | ${word()} | ${sentence()} |`)].join('\n')
  const formula = () => `$$\n\\sum_{k=1}^{${2 + pick(8)}} \\frac{${word()}_k}{${1 + pick(9)}} = \\int_0^1 x^{${pick(5)}}\\,dx\n$$`
  return {
    random,
    pick,
    sentence,
    paragraph,
    items,
    code,
    table,
    formula,
    /** Paragraphs only, `count` of them: text that only grows at its end as it streams. */
    prose: (count: number) => Array.from({ length: count }, () => paragraph()).join('\n\n'),
    /** An answer of `count` parts of every shape: paragraphs, a heading, a list, a code block, a table. */
    answer: (count: number) =>
      Array.from({ length: count }, (_, part) => {
        switch (part % 6) {
          case 1:
            return `## ${sentence().slice(0, -1)}\n\n${paragraph()}`
          case 2:
            return items()
          case 3:
            return code()
          case 5:
            return table()
          default:
            return paragraph()
        }
      }).join('\n\n'),
  }
}
