// The slot's scripted model (browse.md § The scripted model): an
// Anthropic-compatible Messages endpoint on the slot's model port, which
// the slot's backend calls as it calls a vendor. It answers each request of a
// turn with the next answer a check queued, at the pace the answer names, so
// a check decides what a conversation's transcript holds and when each part
// of it arrives, with the backend, the agent and the page's connection all
// the product's own.
//
// ```text
//   check: demi.model.answers([...])
//                |
//                v
//   page --> backend --> POST /v1/messages --> next answer, as the
//    ^          |                              vendor's event stream
//    +----------+  the transcript's changes
// ```
//
// - An answer is the model's whole reply to one request: its steps in order,
//   thinking, text and at most one command, which ends the answer as a tool
//   call. The backend runs the command and asks again, which takes the next
//   answer.
// - A request with no tools is no step of a turn, such as one for a
//   conversation's title: it takes no answer and gets a fixed line.
// - A turn's request that finds no answer queued gets one line of text, so a
//   turn always ends.
// - An answer whose command the request offers no tool for is refused before
//   its stream begins, with a 400 the backend does not try again, and stays
//   queued: a stream that broke midway would be tried again, and the try
//   would take the next answer.
// - Its usage is Demi's own estimate, a token for each 4 bytes: the request's
//   JSON as its input, the answer's text as its output, so the context
//   indicator and compaction see a conversation the size it is.
// - The queue lives as long as the tool's server. A server that starts again
//   for changed code has none.
import type { Server } from 'bun'

/** One step of an answer. */
export type Step =
  /** Thinking, streamed over `ms` milliseconds, at once without. */
  | { think: string, ms?: number }
  /** Text, as Markdown, streamed over `ms` milliseconds, at once without. */
  | { say: string, ms?: number }
  /**
   * A shell command, which ends the answer: the backend runs it and asks
   * again. With `background` the call returns at once and the command runs
   * on; with `service` it returns once the command's output is quiet.
   */
  | { run: string, background?: boolean, service?: boolean }
  /** Nothing for this many milliseconds. */
  | { wait: number }

/** The model's reply to one request. */
export type Answer = Step[]

/** A request the backend made, as a check reads it. */
export interface Asked {
  /** Milliseconds since the Unix epoch. */
  at: number
  /** The names of the tools the request offers; none for a request that is no step of a turn. */
  tools: string[]
  /** The text of the request's last user message, or what its last tool result says. */
  last: string
  /** The request's JSON, all the model reads. */
  body: string
  /** The answer it took; null for the fixed lines. */
  answer: Answer | null
}

/** What a turn's request gets when no answer is queued. */
const NOTHING_QUEUED = 'The scripted model has no answer queued.'
/** What a request that offers no tool gets, such as one for a title. */
const NO_TURN = 'Scripted'
/** The model's name in the stream. */
export const SCRIPTED_MODEL = 'scripted'

/** A tool a request offers the model. */
interface OfferedTool {
  name: string
  input_schema?: { properties?: Record<string, unknown>, required?: string[] }
}

/** The parts of a Messages request the endpoint reads. */
interface MessagesRequest {
  tools?: OfferedTool[]
  messages?: { role: string, content: string | { type: string, text?: string, content?: unknown }[] }[]
}

export class ScriptedModel {
  private queue: Answer[] = []
  private readonly asked: Asked[] = []
  private blocks = 0

  private constructor(private readonly server: Server<undefined>) {}

  /** Listens on `port` of the loopback interface. */
  static listen(port: number): ScriptedModel {
    // Assigned before the first request, which the handler reads it at.
    let model: ScriptedModel | undefined
    const server = Bun.serve({
      hostname: '127.0.0.1',
      port,
      // A stream stays open while its answer waits, as long as the answer says.
      idleTimeout: 0,
      fetch: async (request) => {
        const url = new URL(request.url)
        if (request.method !== 'POST' || url.pathname !== '/v1/messages') {
          return new Response('The scripted model answers POST /v1/messages only', { status: 404 })
        }
        const body = await request.text()
        return model!.answer(body, request.signal)
      },
    })
    model = new ScriptedModel(server)
    return model
  }

  /** The base URL a provider entry names: the backend appends `/messages`. */
  get url(): string {
    return `http://127.0.0.1:${this.server.port}/v1`
  }

  /** Replaces what is queued with `answers`, each for one request of a turn, in order. */
  answers(answers: Answer[]): void {
    this.queue = [...answers]
  }

  /** How many answers no request has taken yet. */
  get left(): number {
    return this.queue.length
  }

  /** The requests since the endpoint listened, or since `forget`. */
  requests(): Asked[] {
    return [...this.asked]
  }

  /** Drops the queue and the requests. */
  forget(): void {
    this.queue = []
    this.asked.length = 0
  }

  async shutdown(): Promise<void> {
    await this.server.stop(true)
  }

  private answer(body: string, gone: AbortSignal): Response {
    const request = JSON.parse(body) as MessagesRequest
    const inputTokens = Math.ceil(Buffer.byteLength(body) / 4)
    const tools = request.tools ?? []
    const turn = tools.length > 0
    const shell = tools.find((tool) => commandProperty(tool) !== null)
    const queued = turn ? this.queue[0] ?? null : null
    if (queued?.some((step) => 'run' in step) && !shell) {
      const message = `The scripted model's next answer runs a command, and the request offers no tool that takes one: ${tools.map((tool) => tool.name).join(', ')}`
      return Response.json({ type: 'error', error: { type: 'invalid_request_error', message } }, { status: 400 })
    }
    const taken = turn ? this.queue.shift() ?? null : null
    this.asked.push({ at: Date.now(), tools: tools.map((tool) => tool.name), last: lastSaid(request), body, answer: taken })
    const steps: Answer = taken ?? [{ say: turn ? NOTHING_QUEUED : NO_TURN }]
    const encoder = new TextEncoder()
    const events = this.events(steps, shell, inputTokens, gone)
    return new Response(
      new ReadableStream<Uint8Array>({
        async pull(controller) {
          const next = await events.next()
          if (next.done) {
            controller.close()
            return
          }
          controller.enqueue(encoder.encode(`event: ${next.value.type}\ndata: ${JSON.stringify(next.value)}\n\n`))
        },
        cancel: () => void events.return(undefined),
      }),
      { headers: { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' } },
    )
  }

  /** The vendor's events for `steps`, each when its step's pace has it; ends early once the backend is `gone`. */
  private async *events(steps: Answer, shell: OfferedTool | undefined, inputTokens: number, gone: AbortSignal): AsyncGenerator<{ type: string } & Record<string, unknown>> {
    const written = steps.reduce((sum, step) => sum + Buffer.byteLength('say' in step ? step.say : 'think' in step ? step.think : 'run' in step ? step.run : ''), 0)
    yield {
      type: 'message_start',
      message: {
        id: `msg_scripted_${Date.now()}`,
        type: 'message',
        role: 'assistant',
        model: SCRIPTED_MODEL,
        content: [],
        stop_reason: null,
        stop_sequence: null,
        usage: { input_tokens: inputTokens, output_tokens: 1 },
      },
    }
    let index = 0
    let stop = 'end_turn'
    for (const step of steps) {
      if (gone.aborted) {
        return
      }
      if ('wait' in step) {
        await Bun.sleep(step.wait)
        continue
      }
      if ('run' in step) {
        // `answer` refused the request before the stream when it offers no such tool.
        if (!shell) {
          return
        }
        yield { type: 'content_block_start', index, content_block: { type: 'tool_use', id: `toolu_scripted_${++this.blocks}`, name: shell.name, input: {} } }
        yield { type: 'content_block_delta', index, delta: { type: 'input_json_delta', partial_json: JSON.stringify(commandInput(shell, step)) } }
        yield { type: 'content_block_stop', index }
        stop = 'tool_use'
        break
      }
      const thinking = 'think' in step
      const text = thinking ? step.think : step.say
      yield {
        type: 'content_block_start',
        index,
        content_block: thinking ? { type: 'thinking', thinking: '', signature: '' } : { type: 'text', text: '' },
      }
      const pieces = step.ms ? chunks(text) : [text]
      const gap = step.ms ? step.ms / pieces.length : 0
      for (const piece of pieces) {
        if (gone.aborted) {
          return
        }
        yield { type: 'content_block_delta', index, delta: thinking ? { type: 'thinking_delta', thinking: piece } : { type: 'text_delta', text: piece } }
        if (gap > 0) {
          await Bun.sleep(gap)
        }
      }
      if (thinking) {
        yield { type: 'content_block_delta', index, delta: { type: 'signature_delta', signature: 'scripted' } }
      }
      yield { type: 'content_block_stop', index }
      index += 1
    }
    yield { type: 'message_delta', delta: { stop_reason: stop, stop_sequence: null }, usage: { output_tokens: Math.max(1, Math.ceil(written / 4)) } }
    yield { type: 'message_stop' }
  }
}

/** The pieces a streamed text arrives in: its words, each with the space after it, a long one in parts. */
function chunks(text: string): string[] {
  /** A word longer than this, as a line of text without spaces is, arrives in parts of it, as a model's tokens would. */
  const part = 12
  return (text.match(/\S+\s*|\s+/g) ?? []).flatMap((word) => word.match(new RegExp(`[\\s\\S]{1,${part}}`, 'g')) ?? [])
}

/** The names of the string properties `tool` takes: those it requires, or all of them where it requires none. */
function stringProperties(tool: OfferedTool): string[] {
  const properties = tool.input_schema?.properties ?? {}
  return (tool.input_schema?.required ?? Object.keys(properties))
    .filter((name) => (properties[name] as { type?: string } | undefined)?.type === 'string')
}

/** The name of the property a tool takes its command in, by the names tools give it; null for a tool that takes none. */
function commandProperty(tool: OfferedTool): string | null {
  return stringProperties(tool).find((name) => /^(command|script|cmd)$/i.test(name)) ?? null
}

/**
 * A call's input that runs `command`. Every other string the tool requires,
 * such as a title of the step, says the command's first line, as a model
 * that describes its step briefly would.
 */
function commandInput(tool: OfferedTool, step: Extract<Step, { run: string }>): Record<string, string | boolean> {
  const title = step.run.split('\n')[0]!
  const input: Record<string, string | boolean> = Object.fromEntries(stringProperties(tool).map((name) => [name, title]))
  input[commandProperty(tool)!] = step.run
  if (step.background) {
    input.background = true
  }
  if (step.service) {
    input.service = true
  }
  return input
}

/** What the request's last message says: a user's text, or a tool result's. */
function lastSaid(request: MessagesRequest): string {
  const last = request.messages?.at(-1)
  if (!last) {
    return ''
  }
  if (typeof last.content === 'string') {
    return last.content
  }
  return last.content
    .map((block) => block.text ?? (typeof block.content === 'string' ? block.content : JSON.stringify(block.content ?? '')))
    .join('\n')
}
