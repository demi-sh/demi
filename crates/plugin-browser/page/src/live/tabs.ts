/**
 * One conversation's browser on the page, which the plugin's store keeps
 * while the plugin is on (`../index.ts`). It holds:
 *
 * - what a tab keeps;
 * - the conversation browser's tab list, which the plugin's conversation state
 *   brings;
 * - the tabs the user opens, until the work has them;
 * - the requests that move a tab, and the user's latest one on each;
 * - the panel's size;
 * - each browser tab as a view last reported it, and its last picture;
 * - the one view a page keeps, watched while a `browser` tab is shown and the
 *   page is visible, and kept unwatched otherwise.
 *
 * The work's `browser` tabs are the browser tabs the device lists, as the
 * plugin mirrors them on the backend: the session opens a tab only by asking
 * the device (`open`), and never adds, changes or closes a work tab.
 *
 * Why it is built so:
 *
 * - The browser has a tab only once its tab list names it: a work tab names
 *   the device's last list, which a browser starting again has not opened
 *   yet.
 * - A request shows its loading before it leaves the page and ends only on a
 *   tab list numbered after it (the device's numbers only grow), whatever order
 *   answer and lists arrive in.
 * - The view stays open while a shown tab opens, so the device learns the panel's
 *   size before it creates the tab.
 * - A page that cannot decode H.264 opens no view, since it would lower the
 *   picture for every viewer with missing acknowledgements and decide the tab's
 *   size with blind clicks.
 * - A view nobody watches, as when another tab is selected, its window closes,
 *   the conversation changes or the page hides, stays open watching no tab:
 *   the device stops capturing, the stream says it is unwatched so nothing stays
 *   awake for it, and shown again it watches at once on the same connection,
 *   over the tab's last picture.
 */
import { clientPlatform } from '@demicodes/utils'
import { useDocumentVisibility, useDebounceFn } from '@vueuse/core'
import { computed, shallowReactive, shallowRef, watch, type ComputedRef, type Ref, type ShallowRef } from 'vue'
import { z } from 'zod'
import { controlReasonSchema, type BrowserTabs, type LiveDownload, type LiveTab, type ControlReason, type TabOpened } from '../generated/plugin'
import { viewerClipboard } from './clipboard'
import { picturesSupported } from '@demicodes/plugin-sdk'
import type { DeviceDownload, OpenUserStream, PageContext, SentenceText } from '@demicodes/plugin-sdk'
import { LiveSession, stalled, type LiveStream, type PanelReport } from './session'
import { t } from '../locales/text'

/** The address of a blank page, which a new tab shows. */
export const BLANK_URL = 'about:blank'

/** The id of the work tab the plugin adds for the browser's tab `tab`. */
export function addedWorkTab(tab: string): string {
  return `browser-${tab}`
}

/**
 * How long after the viewer's click or key in a tab a tab that page opens is
 * the viewer's, which a browser selects: the window in which the device takes
 * text the page copies as the viewer's.
 */
const OPENED_BY_USER_MS = 5000

/** What a `browser` tab keeps: its browser tab as the device last listed it, which the plugin writes. */
export const browserTabDataSchema = z.object({
  /** The conversation browser's tab it shows. */
  tab: z.string(),
  /** The address of its page. */
  url: z.string(),
  /** Its page's title, so the strip names the tab while no view shows it. */
  title: z.string().optional(),
  /** How many times the agent showed the browser tab; absent while it never did. */
  shows: z.number().int().min(1).optional(),
  /**
   * The work tab whose page opened it, by a link or script of the page,
   * which the plugin writes as it adds the tab.
   */
  openedBy: z.string().optional(),
})
export type BrowserTabData = z.infer<typeof browserTabDataSchema>

/** The plugin's conversation state: the conversation browser's tabs. */
export type BrowserTabList = BrowserTabs

/**
 * What every reason the page can be told means to the user, in the Writing
 * page's words rather than the device's error text: the browser's own codes, the
 * view's capture notices, the conversation's device access's refusals and the
 * plugin call route's own. The catalogue's `reasons` hold a sentence for each,
 * so a reason without one does not build.
 */
export function reasonSentence(reason: ControlReason): SentenceText {
  return t(`reasons.${reason}`)
}

/** What a failure with no reason says when the backend answered with `status`, such as `500 Internal Server Error`. */
export function serverRefused(status: string): SentenceText {
  return t('answers.refused', { status })
}

/**
 * What a request failed for: a reason of the browser's or the backend's, the
 * status of an answer without one, or that no answer came (a network
 * failure, a timeout) or that the answer could not be read.
 */
export type TabsFailure =
  | { reason: ControlReason }
  | { status: string }
  | { answer: 'none' | 'unreadable' }

/** The page's words for a failure, in the page's language as of now. */
function tabsFailureSentence(failure: TabsFailure): SentenceText {
  if ('reason' in failure) {
    return reasonSentence(failure.reason)
  }
  return 'status' in failure ? serverRefused(failure.status) : t(`answers.${failure.answer}`)
}

/**
 * A request that failed, keeping what it failed for. Its `message` is worded as
 * it is read, from the failure, so a language chosen while it shows rewords it,
 * as a `ShownError` does. A request never fails for the page's connection: the
 * shell's call waits for the backend (`packages/web-ui/src/plugins/page.ts`).
 */
export class BrowserTabsError extends Error {
  /** The backend's or the browser's reason; null for a failure that carries none. */
  readonly reason: ControlReason | null

  constructor(readonly failure: TabsFailure) {
    super()
    this.reason = 'reason' in failure ? failure.reason : null
    // `message` is a property of Error, which an accessor cannot override in a subclass.
    Object.defineProperty(this, 'message', { get: () => tabsFailureSentence(this.failure), configurable: true })
  }

  static of(reason: ControlReason): BrowserTabsError {
    return new BrowserTabsError({ reason })
  }
}

/**
 * The conversation browser's tab list and tab methods and its user stream, as
 * the plugin's page context supplies them. Every request rejects with a
 * `BrowserTabsError`, except one dropped as its browser was released, which
 * rejects with the abort.
 */
export interface BrowserTabsApi {
  /** The tab list, as the plugin's conversation state follows it. */
  tabs: {
    /** The list last read; null before the first. */
    value: Readonly<Ref<BrowserTabList | null>>
    /** Why the last read failed, until one succeeds. */
    error: Readonly<Ref<BrowserTabsError | null>>
  }
  /**
   * Has the device open a tab of the conversation's browser, on `url` or
   * blank, the user's, which the device shows; answers the browser tab
   * once the work holds it.
   */
  open(url: string | null): Promise<TabOpened>
  /** Brings the window of the work tab `workTab` to the front on it, once the work has it. */
  select(workTab: string): void
  /** Starts loading `url`; answers the number of the last tab list before the request started. */
  navigate(tab: string, url: string): Promise<number>
  /** Moves or reloads the tab; answers as `navigate` does. */
  history(tab: string, action: 'back' | 'forward' | 'reload'): Promise<number>
  /** Stops loading the tab's page; answers as `navigate` does. */
  stop(tab: string): Promise<number>
  stream: OpenUserStream
  /** Whether the device is a Cloud that does not run yet, read reactively. */
  deviceStarting(): boolean
  /** What the device downloads of the plugin's packages now, Chrome or its program, read reactively. */
  downloading(): readonly DeviceDownload[]
}

/** Answers that will not change by asking again. */
const FINAL_REASONS: ReadonlySet<ControlReason | null> = new Set<ControlReason>(['conversation_not_found', 'conversation_archived'])

/** Whether this web browser can show the view's pictures, once it has answered. */
export type PictureSupport = 'checking' | 'supported' | 'unsupported'

/**
 * What Demi does before the browser has a tab, one the user opens or one a
 * browser starting again opens anew: reads what the device is doing, starts
 * the Cloud, downloads the browser or its program to the device, starts the
 * browser on the device, or has the browser open the tab.
 */
export type StartingPhase = 'connecting' | 'cloud' | 'download' | 'browser' | 'page'

/** What the content says in each phase, from the frame the phase begins. */
export function startingLabel(phase: StartingPhase): SentenceText {
  return t(`starting.${phase}`)
}

export interface BrowserTabsOptions {
  /** The page's visibility; the document's own unless a test supplies one. */
  visibility?: Readonly<Ref<DocumentVisibilityState>>
  /** Resolves whether this web browser can decode the view's pictures; WebCodecs' answer unless the gallery or a test supplies one. */
  pictures?: () => Promise<boolean>
}

/**
 * The user's latest request on a browser tab, with the address it asks for,
 * if it names one: sent and not answered yet, or answered with the number of
 * the last tab list before it started, which only a list numbered higher
 * outdates. A refused request leaves none, and one the browser carried out
 * leaves with the list that says so (`settle`).
 */
type TabRequest =
  | { status: 'asked'; url: string | null }
  | { status: 'answered'; list: number; url: string | null }

/**
 * A tab the user opens, on the address it goes to, or none for a blank one:
 * asked, then answered with the browser tab the device opened, until the
 * work has that tab.
 */
export interface Opening {
  id: number
  url: string | null
  opened: TabOpened | null
}

/** A browser tab's last picture, copied off the canvas that showed it, with the generation it was of, which places it. */
export interface KeptPicture {
  image: CanvasImageSource & { width: number; height: number }
  stream: LiveStream
}

/** How the page reports failures: to the user in a toast, or a defect of the page to the console. */
export type PageErrors = PageContext['errors']

/** Whether the page is visible: one listener, for the page's lifetime, that every controller shares. */
const pageVisibility = useDocumentVisibility()

/** How long a panel that resizes waits before the module hears of it: a drag would otherwise resize the capture each step. */
const PANEL_SETTLE_MS = 100

/**
 * One conversation's browser, for one page, made in the effect scope its
 * store gives it. It holds the tab list, each browser tab as a view last
 * reported it with its last picture, the panel the shown tab is sized by, and
 * the view, watched while a `browser` tab's content is shown and the page is
 * visible. It alone tells the view which tab to watch.
 */
export class BrowserTabsController {
  /** The last list the plugin's state gave; null before the first. */
  readonly list: ComputedRef<BrowserTabList | null>
  /** Why the last list could not be read, until one is. */
  readonly listError: ComputedRef<BrowserTabsError | null>
  readonly session: ShallowRef<LiveSession | null> = shallowRef(null)
  /**
   * Whether this web browser can show the pictures, asked once. One that
   * cannot opens no view, so it is no viewer.
   */
  readonly pictures: ShallowRef<PictureSupport> = shallowRef('checking')
  /**
   * Each browser tab as a view last reported it, kept while no view is open,
   * so a tab shown again shows its address, loading and viewport at once.
   * A tab leaves once a view reports the browser without it.
   */
  private readonly known = shallowReactive(new Map<string, LiveTab>())
  /** The number of the last tab list a view reported, in the device's sequence; null before the first. */
  private readonly listed = shallowRef<number | null>(null)
  /** The tabs the user opens, in the order asked, until the work has each (`held`). */
  readonly openings: ShallowRef<readonly Opening[]> = shallowRef([])
  /**
   * How many openings ended without a tab, refused or closed before the
   * work showed it, which a window that holds no tab closes on.
   */
  readonly unopened = shallowRef(0)
  /** The id of the latest opening. */
  private opened = 0
  /** The user's latest request on each browser tab. */
  private readonly requests = shallowReactive(new Map<string, TabRequest>())
  /** The browser tabs the page showed a picture of, while the browser has them. */
  private readonly pictured = shallowReactive(new Set<string>())
  /** Each browser tab's last picture, while the browser has the tab. */
  private readonly lastPictures = new Map<string, KeptPicture>()
  /** The downloads the user started in each browser tab, as a view last reported them, the newest last. */
  private readonly downloads = shallowReactive(new Map<string, readonly LiveDownload[]>())
  /** Every browser tab a view listed; null before the first list, whose tabs are not new to the page. */
  private heard: Set<string> | null = null
  /** The panel the shown tab's content measured, which a view sizes the tab by. */
  private panel: PanelReport | null = null
  /**
   * The work tab whose content is shown, with its browser tab, which the
   * view watches while the page is visible; a content shown before the
   * browser has its tab keeps the view open watching nothing, so the device
   * knows the panel's size for the tab it opens.
   */
  private readonly shownContent = shallowRef<{ workTab: string; tab: string | null } | null>(null)
  /** The browser tab whose content is shown. */
  private readonly shownTab = computed(() => this.shownContent.value?.tab ?? null)
  private closing: ReturnType<typeof setTimeout> | null = null
  private readonly visibility: Readonly<Ref<DocumentVisibilityState>>
  private disposed = false
  /** Tells the view a panel that stopped resizing for a moment; after the view closed, nobody. */
  private readonly settled = useDebounceFn(() => this.session.value?.panel(), PANEL_SETTLE_MS)

  constructor(
    readonly api: BrowserTabsApi,
    private readonly errors: PageErrors,
    options: BrowserTabsOptions = {},
  ) {
    this.visibility = options.visibility ?? pageVisibility
    this.list = computed(() => api.tabs.value.value)
    this.listError = computed(() => api.tabs.error.value)
    // The watchers stop with the effect scope its store made it in.
    watch(this.visibility, (state) => this.visibilityChanged(state), { flush: 'sync' })
    // A view the stopped Cloud refused connects at once once the Cloud runs, so
    // the device knows the panel's size before it opens the tab a shown content
    // waits for.
    watch(() => api.deviceStarting(), (starting, was) => {
      if (was && !starting) {
        this.session.value?.reconnect()
      }
    })
    watch(this.listError, (error) => {
      if (error && FINAL_REASONS.has(error.reason)) {
        this.closeView()
      }
    })
    watch(this.list, (listed) => {
      if (listed) {
        this.unlisted(listed)
      }
    })
    const pictures = options.pictures ?? (() => picturesSupported(errors.defect))
    void pictures().then((supported) => {
      if (this.disposed) {
        return
      }
      this.pictures.value = supported ? 'supported' : 'unsupported'
      this.watchShown()
    })
  }

  /** The browser tab `tab` as a view last reported it, if one did. */
  tab(tab: string | undefined): LiveTab | null {
    return tab === undefined ? null : (this.known.get(tab) ?? null)
  }

  /** The last picture a view of the browser tab `tab` showed, kept for the next view of it. */
  keepPicture(tab: string, picture: KeptPicture): void {
    if (this.known.has(tab)) {
      this.lastPictures.set(tab, picture)
    }
  }

  /** The browser tab's last picture, if a view kept one. */
  keptPicture(tab: string): KeptPicture | null {
    return this.lastPictures.get(tab) ?? null
  }

  /** The downloads the user started in the browser tab `tab`, the newest last. */
  downloadsOf(tab: string | undefined): readonly LiveDownload[] {
    return tab === undefined ? [] : (this.downloads.get(tab) ?? [])
  }

  /**
   * Whether the browser tab `tab`, on `url`, shows its page loading.
   *
   * - A request of the user's loads from the moment it is made until a tab list
   *   numbered higher than the one its answer names says otherwise, whichever
   *   reaches the page first. On a far backend the browser may start loading
   *   well after the answer, and a list numbered no higher describes the page
   *   as it was.
   * - Otherwise the page loads while the browser last said it does: in a view,
   *   or, for a tab not shown, in the plugin's tab list.
   * - A shown tab no view reported yet loads while it is on an address and
   *   the view does not wait for the device.
   * - A tab shown again keeps what the browser last said, so a page that had
   *   loaded shows no loading.
   */
  loading(tab: string, url: string): boolean {
    if (this.requested(tab)) {
      return true
    }
    const live = this.tab(tab)
    if (live) {
      return live.loading
    }
    // A tab the strip shows beside the shown one, which no view reported, as the plugin last listed it.
    if (tab !== this.shownTab.value) {
      return this.listedTab(tab)?.loading ?? false
    }
    return url !== BLANK_URL && !this.waiting()
  }

  /**
   * Whether the page's view waits for the device, as while a stopped Cloud
   * refuses it: the tabs stay as the device last listed them, and say so.
   */
  waiting(): boolean {
    return stalled(this.session.value?.state)
  }

  /**
   * Whether a request of the user's on `tab` is under way: sent, or answered
   * while no tab list numbered after the answer reached the page. Until then
   * what the browser reports of the tab describes the page the request
   * leaves.
   */
  private requested(tab: string): boolean {
    const request = this.requests.get(tab)
    if (request?.status === 'asked') {
      return true
    }
    const listed = this.listed.value
    return request?.status === 'answered' && (listed === null || listed <= request.list)
  }

  /** The browser tab `tab` as the plugin last listed it, if it did. */
  private listedTab(tab: string): BrowserTabList['tabs'][number] | null {
    return this.list.value?.tabs.find((candidate) => candidate.id === tab) ?? null
  }

  /**
   * The address the user's latest request on the browser tab `tab` asks
   * for, while the browser still reports the page the request leaves:
   * until a list after the request, while the browser loads from another
   * address, as it does until the new page commits, and while it reports
   * the blank page of a tab that has not started on the address yet. None
   * for a request that names no address, as Back does.
   */
  private asked(tab: string): string | null {
    return this.requests.get(tab)?.url ?? null
  }

  /**
   * What the browser reports of the page the work tab with `data` shows: as
   * a view last reported it, else as the plugin last listed it, else as the
   * work tab keeps it. The tab's address and its title both come from this
   * one record, so they never describe two pages.
   */
  private reportedPage(data: BrowserTabData): Pick<LiveTab, 'url' | 'title'> {
    return this.tab(data.tab) ?? this.listedTab(data.tab) ?? { url: data.url, title: data.title ?? '' }
  }

  /**
   * The title of the page the work tab with `data` shows, as the browser
   * reports it, the moment it names it, as a web browser's tab takes its
   * page's title in the background too; none when the page has no title, and
   * while a request of the user's leaves the page, as the tab is named after
   * where it goes.
   */
  title(data: BrowserTabData): string | null {
    return this.asked(data.tab) === null ? this.reportedPage(data).title || null : null
  }

  /** The address of the page the work tab with `data` shows: the one a request of the user's goes to, else the page's own. */
  address(data: BrowserTabData): string {
    return this.asked(data.tab) ?? this.reportedPage(data).url
  }

  /**
   * The icon of the page the work tab with `data` shows, as a web
   * browser's tab shows it: as a view last reported it, else as the plugin
   * last listed it; null for a page without one.
   */
  favicon(data: BrowserTabData): string | null {
    return (this.tab(data.tab) ?? this.listedTab(data.tab))?.favicon ?? null
  }

  /**
   * Whether the work tab with `data` shows its page loading: the one state
   * its strip's spinner, its Stop and its progress line show. It loads as its
   * browser tab does, and a shown tab that has no picture yet loads until its
   * first picture, unless none can come or the tab is blank.
   */
  busy(data: BrowserTabData): boolean {
    return this.loading(data.tab, this.address(data)) || (!this.blank(data) && this.awaitsPicture(data.tab))
  }

  /**
   * Whether the work tab with `data` is blank: on the blank page, with no
   * request of the user's under way. It loads nothing, so it shows a blank
   * page, not what Demi waits for; only the browser's own word that its page
   * loads shows it loading.
   */
  private blank(data: BrowserTabData): boolean {
    return this.address(data) === BLANK_URL && !this.requested(data.tab)
  }

  /**
   * Whether the browser tab `tab` is shown with no picture yet, which the
   * view will bring: not in a web browser that cannot show the pictures, nor
   * while the device says it cannot capture the tab, nor while the view
   * waits for the device.
   */
  private awaitsPicture(tab: string): boolean {
    return tab === this.shownTab.value
      && !this.pictured.has(tab)
      && this.pictures.value !== 'unsupported'
      && !this.session.value?.state.pictureless
      && !this.waiting()
  }

  /**
   * What Demi does for the work tab with `data` while the browser does not
   * name its browser tab, as while a browser that starts again opens the
   * tabs of the one before. Null once it names it, and for a blank tab,
   * which waits for nothing the user asked for.
   */
  startingPhase(data: BrowserTabData): StartingPhase | null {
    if (this.blank(data)) {
      return null
    }
    const tabs = this.browserTabs()
    return tabs?.some((tab) => tab.id === data.tab) ? null : this.phase(tabs)
  }

  /**
   * What Demi does for the latest tab the user opens, while one opens; a
   * blank one the browser opens says nothing, as it shows a blank page.
   */
  openingPhase(): StartingPhase | null {
    const latest = this.openings.value.at(-1)
    if (!latest) {
      return null
    }
    const phase = this.phase(this.browserTabs())
    return phase === 'page' && latest.url === null ? null : phase
  }

  /**
   * What Demi does before the browser has a tab, as the conversation's
   * browser holds `tabs`, in order:
   *
   * 1. The Cloud starting, which the product state says.
   * 2. The browser downloading to the device.
   * 3. Connecting, while neither a view nor the plugin's list has said what the
   *    conversation's browser holds.
   * 4. The browser starting, while it has no tabs.
   * 5. The browser opening the tab.
   */
  private phase(tabs: readonly { id: string }[] | null): StartingPhase {
    if (this.api.deviceStarting()) {
      return 'cloud'
    }
    if (this.api.downloading().length > 0) {
      return 'download'
    }
    if (tabs === null) {
      return 'connecting'
    }
    return tabs.length > 0 ? 'page' : 'browser'
  }

  /**
   * The conversation browser's tabs: as the view last reported them while it
   * is connected, else as the plugin last listed them; null while neither
   * has said what the browser holds.
   */
  private browserTabs(): readonly { id: string }[] | null {
    const view = this.session.value?.state
    if (view?.connection === 'live' || view?.connection === 'stalled') {
      return view.running ? view.tabs : []
    }
    return this.list.value?.tabs ?? null
  }

  /**
   * Opens a tab of the conversation's browser on `url`, or blank, as the
   * window's + and a link's Open in Browser Window do: the device shows it,
   * so the work selects it once it has it. It is an opening until then
   * (`held`). One that ends without a tab is counted in `unopened`: a
   * refusal is a toast, and a tab that closed before the work showed it
   * says nothing, as it is gone.
   */
  async open(url: string | null): Promise<void> {
    this.opened += 1
    const id = this.opened
    this.openings.value = [...this.openings.value, { id, url, opened: null }]
    let opened: TabOpened
    try {
      opened = await this.api.open(url)
    } catch (error) {
      this.openings.value = this.openings.value.filter((each) => each.id !== id)
      // One dropped as its browser was released is no refusal.
      if (!this.disposed) {
        this.unopened.value += 1
        if (!(error instanceof BrowserTabsError && error.reason === 'tab_not_found')) {
          this.errors.report(t('failures.openPage'), error)
        }
      }
      return
    }
    const now = this.openings.value.find((each) => each.id === id)
    if (!now) {
      return
    }
    this.openings.value = this.openings.value.map((each) => (each === now ? { ...now, opened } : each))
    // An address the user typed meanwhile.
    if (now.url !== url && now.url !== null) {
      this.goTo(opened.tab, now.url)
    }
  }

  /**
   * Ends each answered opening whose tab the device's list `listed` no
   * longer names, numbered as high as its answer's or higher: the tab
   * closed before the work showed it.
   */
  private unlisted(listed: BrowserTabList): void {
    const gone = this.openings.value.filter(({ opened }) =>
      opened !== null && listed.list >= opened.list && !listed.tabs.some((tab) => tab.id === opened.tab))
    if (gone.length > 0) {
      this.openings.value = this.openings.value.filter((each) => !gone.includes(each))
      this.unopened.value += 1
    }
  }

  /**
   * The user's address for the latest tab that opens, typed before the work
   * has it: it shows at once, and the tab goes there once the device opened
   * it.
   */
  goOpening(url: string): void {
    const latest = this.openings.value.at(-1)
    if (!latest) {
      return
    }
    this.openings.value = this.openings.value.map((each) => (each === latest ? { ...latest, url } : each))
    if (latest.opened !== null) {
      this.goTo(latest.opened.tab, url)
    }
  }

  /** The user's address, loaded in `tab`; a refusal is a toast. */
  private goTo(tab: string, url: string): void {
    this.navigate(tab, url).catch((error: unknown) => this.errors.report(t('failures.openAddress'), error))
  }

  /** The browser tabs the work holds now: their openings are done. */
  held(tabs: ReadonlySet<string>): void {
    const done = ({ opened }: Opening) => opened !== null && tabs.has(opened.tab)
    if (this.openings.value.some(done)) {
      this.openings.value = this.openings.value.filter((each) => !done(each))
    }
  }

  /** The user's address, loaded in `tab`; rejects with what refused it. */
  navigate(tab: string, url: string): Promise<void> {
    return this.request(tab, url, () => this.api.navigate(tab, url))
  }

  /** The user's Back, Forward or Reload on `tab`; rejects with what refused it. */
  history(tab: string, action: 'back' | 'forward' | 'reload'): Promise<void> {
    return this.request(tab, null, () => this.api.history(tab, action))
  }

  /** The tab menu's Reload of the work tab with `data`, as the address bar's Reload does it; a refusal is a toast. */
  reload(data: BrowserTabData): void {
    this.history(data.tab, 'reload').catch((error: unknown) => this.errors.report(t('failures.reloadPage'), error))
  }

  /**
   * The user's Stop on `tab`. The tab loads until a tab list numbered after
   * the answer says it stopped, as after any request; rejects with what
   * refused it.
   */
  stop(tab: string): Promise<void> {
    return this.request(tab, null, () => this.api.stop(tab))
  }

  /**
   * Runs a request of the user's on `tab`, for the address `url` when it
   * names one. The latest one on the tab is the one its loading and its
   * address follow; a refusal ends it at once, and the caller reports it. A
   * tab the browser no longer has says nothing: the device lists the
   * browser without it, and the work follows.
   */
  private async request(tab: string, url: string | null, run: () => Promise<number>): Promise<void> {
    const asked: TabRequest = { status: 'asked', url }
    this.requests.set(tab, asked)
    let list: number
    try {
      list = await run()
    } catch (error) {
      if (this.requests.get(tab) === asked) {
        this.requests.delete(tab)
      }
      if (error instanceof BrowserTabsError && error.reason === 'tab_not_found') {
        return
      }
      throw error
    }
    // A later request on the tab replaced this one.
    if (this.requests.get(tab) === asked) {
      this.requests.set(tab, { status: 'answered', list, url })
    }
  }

  /**
   * Drops each answered request a tab list numbered `list`, after its
   * answer, shows carried out: the browser no longer has its tab, went to the
   * address it asked for, or stopped loading on a page that is not the blank
   * one it starts on.
   */
  private settle(tabs: readonly LiveTab[], list: number): void {
    for (const [id, request] of [...this.requests]) {
      if (request.status !== 'answered' || list <= request.list) {
        continue
      }
      const tab = tabs.find((candidate) => candidate.id === id)
      if (!tab || request.url === null || tab.url === request.url || (!tab.loading && tab.url !== BLANK_URL)) {
        this.requests.delete(id)
      }
    }
  }

  /**
   * The shown content measured its panel. The first measure goes to the view
   * at once, as a capture starts at it; later ones once the panel stopped
   * resizing for a moment.
   */
  resize(panel: PanelReport): void {
    const first = this.panel === null
    this.panel = panel
    if (first) {
      this.watchShown()
      return
    }
    void this.settled()
  }

  /**
   * A shown content watches its tab on the page's one view, opening the view
   * when there is none.
   *
   * - One shown before the browser has its tab watches nothing, and the view
   *   tells the device the panel's size meanwhile.
   * - A view waiting to reconnect connects at once for a tab it did not show,
   *   as a tab that just got its browser tab: the view's end may have been only
   *   that there was no browser yet.
   * - A hidden page opens no view until it is shown, and a web browser that
   *   cannot show the pictures opens none at all.
   */
  show(workTab: string, tab: string | null): void {
    const another = this.shownTab.value !== tab
    this.shownContent.value = { workTab, tab }
    if (this.closing !== null) {
      clearTimeout(this.closing)
      this.closing = null
    }
    if (another && tab !== null) {
      this.session.value?.reconnect()
    }
    this.watchShown()
  }

  /**
   * The content of `workTab` stops showing. Selecting another `browser` tab
   * shows it in the same flush, before or after this, so the view is
   * unwatched only when none followed.
   */
  hide(workTab: string): void {
    if (this.shownContent.value?.workTab !== workTab) {
      return
    }
    this.shownContent.value = null
    if (this.closing !== null) {
      return
    }
    this.closing = setTimeout(() => {
      this.closing = null
      if (this.shownContent.value === null) {
        this.unwatchView()
      }
    }, 0)
  }

  /**
   * Nobody can watch a hidden page, so the view is unwatched while the page
   * is hidden. Shown again, the page watches the shown tab on it.
   */
  private visibilityChanged(state: DocumentVisibilityState): void {
    if (state !== 'visible') {
      this.unwatchView()
      return
    }
    this.watchShown()
  }

  /** The view watches no tab and says nobody watches it, keeping its connection. */
  private unwatchView(): void {
    const view = this.session.value
    view?.watch(null)
    view?.watched(false)
  }

  /**
   * The shown tab on the page's one view, while someone can see its
   * pictures, once its content measured the panel the tab is sized by.
   */
  private watchShown(): void {
    if (
      this.shownContent.value === null
      || this.panel === null
      || this.visibility.value !== 'visible'
      || this.pictures.value !== 'supported'
    ) {
      return
    }
    const view = this.view()
    view.watched(true)
    view.watch(this.shownTab.value)
  }

  /**
   * The view's tab list. Each tab is kept as reported, and one the browser no
   * longer has leaves. The work's tabs follow the plugin, which the browser
   * tells of every change.
   */
  private viewTabs(tabs: readonly LiveTab[], list: number): void {
    // A browser the next resident starts numbers its lists from its own
    // start: the requests went to the browser before it.
    if (this.listed.value !== null && list < this.listed.value) {
      this.requests.clear()
    }
    this.listed.value = list
    this.settle(tabs, list)
    this.pageOpened(tabs)
    for (const id of [...this.known.keys()]) {
      if (!tabs.some((tab) => tab.id === id)) {
        this.known.delete(id)
        this.pictured.delete(id)
        this.lastPictures.delete(id)
      }
    }
    for (const tab of tabs) {
      this.known.set(tab.id, tab)
    }
    // A state replaces the page's copy: the view tells the downloads of the tab
    // it watches again, and the page keeps none of a tab that is gone or that
    // no view watches.
    const watched = this.shownTab.value
    for (const id of [...this.downloads.keys()]) {
      if (id !== watched || !tabs.some((tab) => tab.id === id)) {
        this.downloads.delete(id)
      }
    }
  }

  /**
   * A tab a page opened appears in the strip at once, as in any browser: the
   * plugin adds it when the browser tells it the tabs changed. One that the
   * page the user watches opened right after the user's click or key there is
   * the user's, and is selected, as a browser selects the tab a click opens.
   * One a page opened by itself is only added.
   */
  private pageOpened(tabs: readonly LiveTab[]): void {
    const heard = this.heard
    this.heard = new Set([...(heard ?? []), ...tabs.map((tab) => tab.id)])
    if (heard === null) {
      return
    }
    const opened = tabs.filter((tab) => tab.createdBy.kind === 'page' && !heard.has(tab.id))
    if (opened.length === 0) {
      return
    }
    const shown = this.shownTab.value
    const view = this.session.value
    for (const tab of opened) {
      const opener = tab.createdBy.kind === 'page' ? tab.createdBy.opener : null
      if (opener !== null && opener === shown && view?.pressedLately(opener, OPENED_BY_USER_MS)) {
        this.api.select(addedWorkTab(tab.id))
      }
    }
  }

  /** The page's one view, opened when there is none. */
  private view(): LiveSession {
    const open = this.session.value
    if (open) {
      return open
    }
    const session = new LiveSession({
      open: this.api.stream,
      panel: () => this.panel,
      platform: clientPlatform(navigator),
      onClipboard: (text) => viewerClipboard.receive(text),
      onTabs: (tabs, list) => this.viewTabs(tabs, list),
      onNotice: (code) => this.notice(code),
      onPicture: (tab) => this.pictured.add(tab),
      onDownloads: (tab, downloads) => this.downloads.set(tab, downloads),
      defect: this.errors.defect,
    })
    this.session.value = session
    session.start()
    return session
  }

  /**
   * A notice the picture still shows through is a failed request: a toast in
   * the Writing page's words for its reason. A tab the browser no longer has
   * says nothing: the device lists the browser without it, and its work tab
   * leaves.
   */
  private notice(reason: ControlReason): void {
    if (reason === 'tab_not_found') {
      return
    }
    this.errors.report(t('failures.operate'), BrowserTabsError.of(reason))
  }

  private closeView(): void {
    this.session.value?.close()
    this.session.value = null
  }

  dispose(): void {
    this.disposed = true
    if (this.closing !== null) {
      clearTimeout(this.closing)
      this.closing = null
    }
    this.shownContent.value = null
    this.known.clear()
    this.requests.clear()
    this.openings.value = []
    this.pictured.clear()
    this.lastPictures.clear()
    this.downloads.clear()
    this.closeView()
  }
}

/** A failure that is no answer of the backend's, such as a network failure or a timeout, as the page says it. */
export function asTabsError(error: unknown): BrowserTabsError {
  return error instanceof BrowserTabsError ? error : new BrowserTabsError({ answer: 'none' })
}
