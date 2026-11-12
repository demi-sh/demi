/**
 * The process a program runs Demi's cores in, `demi-core`, and the HTTP/2
 * connection the SDK speaks to it: on the standard input and output of one
 * the program started, or over TCP to one that runs on its own, `demi-core
 * serve`, with its token (`crates/core-api/src/process/mod.rs`).
 *
 * ```text
 * program                          demi-core
 *   request(GET /cores/x/api/state) --> stream 1
 *   connect(/cores/x/api/connect,      stream 3, CONNECT :protocol websocket
 *           websocket) ---------------> the page connection's frames
 *   connect(/cores/x/api/devices/      stream 5, CONNECT :protocol h2c
 *           d/runner, h2c) -----------> a runner's channel
 * ```
 *
 * A process the program started logs to its standard error, which goes to
 * the program's; its connection's end closes every core, and the process
 * exits. A connection to a process on its own that drops is made again at
 * the next request, and closes nothing.
 *
 * Each connection begins with the program's greeting, which the requests
 * on it wait for: a process on its own that restarted meanwhile knows the
 * program's plugins and cores again before anything else reaches it.
 */
import { spawn } from 'node:child_process'
import http2 from 'node:http2'
import { Duplex, Readable } from 'node:stream'
import { refusal } from './routes'

/** How the process starts. */
export interface ProcessStart {
  /** The `demi-core` executable. */
  executable: string
  /** The release roots whose command packages the cores' runners run. */
  release: readonly string[]
  /** Where the program's server forwards the process's routes. */
  publicUrl?: string
}

/** A process on its own, as a program connects to it. */
export interface ProcessAddress {
  /** Where it listens, such as `http://127.0.0.1:4100`. */
  url: string
  /** The token it was started with, `DEMI_CORE_TOKEN`. */
  token: string
}

/** The headers a stream of HTTP/2 does not carry, which a forwarded request drops. */
const CONNECTION_HEADERS = new Set(['connection', 'host', 'keep-alive', 'proxy-connection', 'transfer-encoding', 'upgrade', 'http2-settings'])

/** An HTTP/2 connection to the process. */
interface Session {
  session: http2.ClientHttp2Session
  /** Resolves once the process's settings came, which say it takes an extended CONNECT. */
  settled: Promise<void>
}

/** What a connection begins with, its requests made on it with `fetch`. */
export type Greeting = (fetch: (request: Request) => Promise<Response>) => Promise<void>

/** A `demi-core`, and the program's connection to it. */
export class Process {
  private session: Session | null = null
  /** Resolves once the connection's greeting is done; fails as it does. */
  private greeted: Promise<void> = Promise.resolve()
  private greeting: Greeting = async () => {}
  /** The streams a CONNECT opened that are still open: the connection's end ends them. */
  private readonly lasting = new Set<http2.ClientHttp2Stream>()

  private constructor(
    private readonly dial: () => Session,
    /** What every request carries besides its own headers: the token. */
    private readonly credentials: http2.OutgoingHttpHeaders,
    /** Whether a connection that ended is made again: one to a process on its own is; one to a child ended with it. */
    private readonly redials: boolean,
    private readonly ending: (session: http2.ClientHttp2Session | null) => Promise<void>,
  ) {}

  /** Starts the process. */
  static start(start: ProcessStart): Process {
    const env: NodeJS.ProcessEnv = { ...process.env, DEMI_RELEASE: start.release.join(pathListSeparator()) }
    if (start.publicUrl !== undefined) {
      env.DEMI_PUBLIC_URL = start.publicUrl
    }
    const child = spawn(start.executable, [], { stdio: ['pipe', 'pipe', 'inherit'], env })
    const exited = new Promise<number | null>((resolve) => {
      child.once('exit', (code) => resolve(code))
      child.once('error', () => resolve(null))
    })
    const dial = (): Session => {
      // Node takes a pair of its own streams, which its types do not say.
      const stdio = Duplex.from({ readable: child.stdout!, writable: child.stdin! } as unknown as Parameters<typeof Duplex.from>[0])
      const session = http2.connect('http://demi-core', { createConnection: () => stdio })
      // Node keeps a session whose socket ended open until its streams
      // close, and a runner's channel on it would stay open with its
      // runner: the process's exit ends them all.
      void exited.then(() => session.destroy())
      // An executable that cannot start, as one that is missing, says why
      // here, and never exits.
      return { session, settled: settled(session, new Promise((_, reject) => child.once('error', reject))) }
    }
    const opened = new Process(dial, {}, false, async (session) => {
      if (session !== null && !session.closed) {
        session.close()
      }
      child.stdin?.end()
      await exited
    })
    return opened
  }

  /** Connects to a process on its own. */
  static connect(address: ProcessAddress): Process {
    const url = new URL(address.url)
    const dial = (): Session => {
      const session = http2.connect(url)
      return { session, settled: settled(session, new Promise(() => {})) }
    }
    return new Process(dial, { authorization: `Bearer ${address.token}` }, true, async (session) => {
      if (session !== null && !session.closed) {
        await new Promise<void>((resolve) => session.close(resolve))
      }
    })
  }

  /**
   * Has every connection begin with `greeting`, from the first, which this
   * makes and answers once it is greeted.
   */
  greet(greeting: Greeting): Promise<void> {
    this.greeting = greeting
    return this.ready()
  }

  /** Resolves once the connection was made and greeted; fails as either does. */
  async ready(): Promise<void> {
    this.current()
    await this.greeted
  }

  /** The connection, made again when one to a process on its own ended. */
  private current(): Session {
    const ended = this.session !== null && (this.session.session.closed || this.session.session.destroyed)
    if (this.session === null || (ended && this.redials)) {
      const made = this.dial()
      this.session = made
      this.greeted = made.settled.then(() => this.greeting((request) => this.send(made, request)))
      // Its failure is each request's to report.
      this.greeted.catch(() => {})
    }
    return this.session
  }

  /**
   * Answers `request`, whose URL's path and query name the process's route,
   * as `fetch` answers one: the response's body streams as it comes.
   */
  async fetch(request: Request): Promise<Response> {
    const made = this.current()
    await this.greeted
    return this.send(made, request)
  }

  /** Answers `request` on the connection `made`. */
  private async send(made: Session, request: Request): Promise<Response> {
    const { session, settled } = made
    await settled
    const url = new URL(request.url)
    const headers: http2.OutgoingHttpHeaders = {
      ':method': request.method,
      ':path': url.pathname + url.search,
    }
    request.headers.forEach((value, name) => {
      if (!CONNECTION_HEADERS.has(name)) {
        headers[name] = value
      }
    })
    Object.assign(headers, this.credentials)
    const stream = session.request(headers, { endStream: request.body === null })
    // A stream that fails closes, which its reader sees; Bun also reports
    // each stream an ended session cancels as an error.
    stream.on('error', () => {})
    if (request.body !== null) {
      const body = Readable.fromWeb(request.body as import('node:stream/web').ReadableStream)
      body.pipe(stream)
      // A stream that closes first, as one the process answered early,
      // cancels what is left of the body.
      stream.once('close', () => body.destroy())
    }
    // The request's signal cancels it until it is answered; then the
    // response's body does, as its reader cancels it. A server such as
    // Deno's aborts the signal once the handler answered, while the body
    // still streams.
    const abort = () => stream.close(http2.constants.NGHTTP2_CANCEL)
    if (request.signal.aborted) {
      abort()
    }
    request.signal.addEventListener('abort', abort, { once: true })
    const answer = await answered(stream)
      .catch((error: unknown) => {
        throw request.signal.aborted ? request.signal.reason : error
      })
      .finally(() => request.signal.removeEventListener('abort', abort))
    const status = Number(answer[':status'])
    const responseHeaders = new Headers()
    for (const [name, value] of Object.entries(answer)) {
      if (name.startsWith(':') || value === undefined) {
        continue
      }
      for (const each of Array.isArray(value) ? value : [value]) {
        responseHeaders.append(name, String(each))
      }
    }
    const empty = request.method === 'HEAD' || status === 204 || status === 304
    if (empty) {
      stream.resume()
    }
    return new Response(empty ? null : webBody(stream), { status, headers: responseHeaders })
  }

  /**
   * Opens `path` by an extended CONNECT of `protocol` (RFC 8441), such as
   * `websocket` or `h2c`, and answers the stream once the process took it.
   */
  async connect(path: string, protocol: string, headers: http2.OutgoingHttpHeaders = {}): Promise<http2.ClientHttp2Stream> {
    const { session } = this.current()
    await this.greeted
    const stream = session.request({
      ...headers,
      ...this.credentials,
      ':method': 'CONNECT',
      ':protocol': protocol,
      ':scheme': 'http',
      ':path': path,
    })
    stream.on('error', () => {})
    // A refusal's body is read as its answer comes: Bun's streams flow
    // before anyone reads them, so the body, which comes with the headers,
    // would be gone once the answer is awaited. An accepted stream is left
    // to its caller untouched.
    const body: Buffer[] = []
    stream.once('response', (headers) => {
      if (Number(headers[':status']) !== 200) {
        stream.on('data', (chunk: Buffer) => body.push(chunk))
      }
    })
    const answer = await answered(stream)
    const status = Number(answer[':status'])
    if (status !== 200) {
      if (!stream.readableEnded && !stream.closed) {
        await new Promise<void>((resolve) => {
          stream.once('end', resolve)
          stream.once('close', resolve)
        })
      }
      throw await refusal(new Response(Buffer.concat(body), { status }), `CONNECT ${path}`)
    }
    this.lasting.add(stream)
    stream.once('close', () => this.lasting.delete(stream))
    return stream
  }

  /**
   * Ends the connection, and the sockets and channels on it: a process the
   * program started closes every core and exits, which this waits for; one
   * on its own closes nothing.
   */
  async close(): Promise<void> {
    for (const stream of this.lasting) {
      stream.close(http2.constants.NGHTTP2_CANCEL)
    }
    await this.ending(this.session?.session ?? null)
  }
}

/**
 * Resolves once `session`'s settings came; fails once it closes or fails
 * first, or `failed` does.
 */
function settled(session: http2.ClientHttp2Session, failed: Promise<never>): Promise<void> {
  const settling = Promise.race([
    new Promise<void>((resolve, reject) => {
      session.once('remoteSettings', () => resolve())
      session.once('error', reject)
      session.once('close', () => reject(new Error('The process ended the connection before it answered')))
    }),
    failed,
  ])
  // Its failure is each request's to report.
  settling.catch(() => {})
  // A failed session ends every request on it, which says why.
  session.on('error', () => {})
  return settling
}

/** The headers `stream` is answered with; throws when it closes unanswered. */
function answered(stream: http2.ClientHttp2Stream): Promise<http2.IncomingHttpHeaders> {
  return new Promise((resolve, reject) => {
    const closed = () => reject(new Error(`The process closed the stream unanswered (${stream.rstCode ?? 'no code'})`))
    stream.once('close', closed)
    stream.once('response', (headers) => {
      stream.off('close', closed)
      resolve(headers)
    })
  })
}

/**
 * The body `stream` brings, as a web stream that reads it as it is read and
 * cancels it once cancelled. Node's `Readable.toWeb` is not used: it throws
 * from its listener when data comes after the reader cancelled, which
 * crashes the program.
 */
function webBody(stream: http2.ClientHttp2Stream): ReadableStream<Uint8Array> {
  let settled = false
  return new ReadableStream<Uint8Array>({
    start(controller) {
      stream.pause()
      stream.on('data', (chunk: Buffer) => {
        if (settled) {
          return
        }
        controller.enqueue(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength))
        if ((controller.desiredSize ?? 0) <= 0) {
          stream.pause()
        }
      })
      stream.once('end', () => {
        if (!settled) {
          settled = true
          controller.close()
        }
      })
      stream.once('close', () => {
        if (!settled) {
          settled = true
          controller.error(new Error(`The process reset the stream (${stream.rstCode})`))
        }
      })
    },
    pull() {
      stream.resume()
    },
    cancel() {
      settled = true
      stream.close(http2.constants.NGHTTP2_CANCEL)
    },
  })
}

/** What separates the paths of a list, as `PATH` writes them. */
function pathListSeparator(): string {
  return process.platform === 'win32' ? ';' : ':'
}
