/**
 * A program's Demi: the process its cores run in, which `Demi.start` starts
 * or `Demi.connect` connects to, with the program's plugins, and the cores
 * it opens.
 */
import { resolve } from 'node:path'
import { openedCoreSchema } from '@demicodes/protocol'
import { binaries } from './binaries'
import { Core } from './core'
import type { Plugin } from './plugin'
import { Process } from './process'
import { below, PROCESS_ORIGIN, refusal } from './routes'

/** What Demi starts with. */
export interface DemiOptions {
  /** The program's plugins, which its cores' agents use besides Demi's own. */
  plugins?: readonly Plugin[]
  /**
   * Where the program's server forwards the process's routes, such as
   * `https://bot.example.com/demi`; without it, the cores offer neither a
   * plugin's sign-in nor a schedule's URL.
   */
  publicUrl?: string
}

/** How Demi connects to a process that runs on its own, `demi-core serve`. */
export interface DemiConnection {
  /** Where the process listens, such as `http://127.0.0.1:4100`. */
  url: string
  /** The token the process was started with, `DEMI_CORE_TOKEN`. */
  token: string
  /**
   * The program's plugins, which the process's cores' agents use besides
   * Demi's own; each replaces the one of its id declared before, as the
   * program's earlier run declared it.
   */
  plugins?: readonly Plugin[]
}

export class Demi {
  private readonly cores = new Map<string, Core>()

  private constructor(
    private readonly process: Process,
    private readonly plugins: readonly Plugin[],
    /** The package's own runner, which a device spawns with no command. */
    private readonly runner: () => string,
    /** Whether the process is the program's own, which ends with it. */
    private readonly owned: boolean,
  ) {}

  /** Starts a process of the program's own, with the program's plugins. */
  static async start(options: DemiOptions = {}): Promise<Demi> {
    const found = binaries()
    const process = Process.start({
      executable: found.core,
      release: found.release,
      ...(options.publicUrl === undefined ? {} : { publicUrl: options.publicUrl }),
    })
    const demi = new Demi(process, options.plugins ?? [], () => found.runner, true)
    await process.greet((fetch) => demi.greet(fetch))
    return demi
  }

  /**
   * Connects to a process that runs on its own, and declares the program's
   * plugins: the cores it holds, their conversations and their turns went
   * on while the program was away. A connection made again, as after the
   * process restarted, declares them again and opens the program's cores
   * again before anything else.
   */
  static async connect(connection: DemiConnection): Promise<Demi> {
    const process = Process.connect({ url: connection.url, token: connection.token })
    // A program that spawns no device needs no executable of its own.
    const demi = new Demi(process, connection.plugins ?? [], () => binaries().runner, false)
    await process.greet((fetch) => demi.greet(fetch))
    return demi
  }

  /**
   * Opens the core in `dir`, creating it when `dir` is absent or empty, and
   * reads its product state, which it follows from then on. A core the
   * process holds already, as one the program's earlier run opened, is the
   * same core.
   */
  async open(dir: string): Promise<Core> {
    const path = resolve(dir)
    const response = await this.call('POST', '/cores', { dir: path })
    const { id } = openedCoreSchema.parse(await response.json())
    const opened = this.cores.get(id)
    if (opened !== undefined) {
      return opened
    }
    const core = new Core(
      { process: this.process, id, dir: path, plugins: this.plugins, runner: this.runner },
      async () => {
        this.cores.delete(id)
        await (await this.call('DELETE', `/cores/${encodeURIComponent(id)}`)).body?.cancel()
      },
    )
    this.cores.set(id, core)
    await core.follow()
    return core
  }

  /**
   * Answers a call of a schedule's URL, `/hooks/{id}` below `base`, as the
   * program's server forwards it.
   */
  fetch(request: Request, base = ''): Promise<Response> {
    const url = new URL(request.url)
    return this.process.fetch(new Request(PROCESS_ORIGIN + below(url.pathname, base) + url.search, request))
  }

  /**
   * Ends the program's use of Demi: a process the program started closes
   * every core and exits; one on its own keeps its cores, which the
   * program's next run opens again.
   */
  async close(): Promise<void> {
    const cores = [...this.cores.values()]
    this.cores.clear()
    if (this.owned) {
      await Promise.all(cores.map((core) => core.close()))
    } else {
      for (const core of cores) {
        core.disconnect()
      }
    }
    await this.process.close()
  }

  /**
   * What each connection to the process begins with: the program's
   * plugins, then its cores, which a process that restarted holds no
   * longer.
   */
  private async greet(fetch: (request: Request) => Promise<Response>): Promise<void> {
    for (const plugin of this.plugins) {
      const response = await this.call('PUT', `/plugins/${encodeURIComponent(plugin.manifest.id)}`, plugin.manifest, fetch)
      await response.body?.cancel()
    }
    for (const core of this.cores.values()) {
      const response = await this.call('POST', '/cores', { dir: core.dir }, fetch)
      await response.body?.cancel()
    }
  }

  private async call(
    method: string,
    path: string,
    body?: unknown,
    fetch = (request: Request) => this.process.fetch(request),
  ): Promise<Response> {
    const response = await fetch(new Request(PROCESS_ORIGIN + path, {
      method,
      ...(body === undefined ? {} : { body: JSON.stringify(body), headers: { 'content-type': 'application/json' } }),
    }))
    if (!response.ok) {
      throw await refusal(response, `${method} ${path}`)
    }
    return response
  }
}
