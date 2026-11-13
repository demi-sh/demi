// `bun browse help` and `jush.help()`: what a script has, and the helpers
// `jush` adds (browse.md).
import type { Tool } from '../tool'

export function help(tool: Tool): void {
  tool.print(HELP)
}

export const HELP = `bun browse: product checks as Playwright scripts in the slot's browser (docs/delivery/browse.md)

  bun browse [--headed] [--limit <seconds|Nm>] <<'JS' … JS   run a script from standard input
  bun browse [--headed] [--limit <seconds|Nm>] <file>          run a script file
  bun browse up [backend] [web] [gallery] | down [servers] [--wipe] | stop | help

A script is the body of an async function. It prints what it console.logs and
what it returns, then the page's state; a call ends after ten minutes unless
--limit says otherwise. It has:
  page      the slot's Playwright Page, open between calls; page.goto('/chat/c-1') opens the web app
  context   its BrowserContext; an action waits 10 s for its element unless it says { timeout }
  expect    Playwright's expect, whose assertions wait until they hold
  cdp       a DevTools protocol session on the page
  keep      an object whose properties outlive the call, for a later call
  jush      the project's helpers:
    jush.up(...servers, { publicHost, built }) start backend, web, gallery (backend and web by default), sign in;
                                         publicHost 'localhost' lets a page at localhost make passkeys
                                         built serves the web app's build through vite preview
    jush.down(...servers)                stop those, or all and the browser; jush.down({ wipe: true }) also removes the data and the runner
    jush.stop()                          close the browser
    jush.shot(name, { page, element, region, pad, zoom, full, now })   save a PNG at the page's real size, of page (such as jush.webkit()'s) or the slot's; answers its path; the one way to picture the slot's browser, where page.screenshot fails
    jush.timeline(action, [0, 100, 1500], { region, name })      frames painted those ms after the action
    jush.frames(probe)                   run probe in the page at every frame it draws; .stop() answers what each read, with when
    jush.focusRings({ root, name, shots })   Tab through the page and focus what Tab skips; report each focus indicator an ancestor clips or an element covers, with px per side, and photograph it
    jush.focusRings.of(locator)           the same for the indicator an element shows now, such as a selected capsule's outline
    jush.pixel(x, y)                     the colour at a point
    jush.net.latency(ms) .bandwidth(kbps | null) .offline() .unreachable() .online() .cut(pattern) .reset() .status()
    jush.ime(text, { into, commit })     compose text through an input method, in the order of into's engine; commit 'enter' | 'escape' | 'none'
    jush.webkit()                        a WebKit (Safari) page for this call, signed in; its goto takes app addresses
    jush.emulate({ viewport, scale, theme, device, locale, timeZone, platform, reset })   without reloading; platform: 'mac' | 'windows' | 'linux' is the system the page reads from its navigator
    jush.hide({ page }) .show({ page })  hide the slot's page or page behind another tab, so it sees visibilityState 'hidden' and visibilitychange, and bring it back; the call's end shows it
    jush.grant(permission, { origin })   such as 'clipboard' or 'local-network-access'
    jush.gallery(path)                   open a gallery page, such as '/session?view=blocks'
    jush.message(text, { files, wait, timeout })   send in the open conversation, with files from this machine, and wait for its turn, not for subagents; fails at once, with the page's words, when it is not delivered or the turn fails, and before writing when the draft holds files
    jush.turn({ timeout })               wait until the open conversation no longer runs, from a message on its way to the subagents and background jobs it started
    jush.model.answers([[{ think, ms }, { say, ms }, { run }, { wait }], …])   script the model's replies to the next requests of a turn, each a list of steps; adds the Scripted model to the backend and chooses it in the composer
    jush.model.requests() .left() .forget()   what the backend asked the scripted model, how many answers are still queued, and drop both
    jush.transcript.watch()              read the transcript in every painted frame; .shown() .grown(px) .still(ms) wait for what it shows, .stop() answers the recording
    jush.transcript.judge(recording)     the recording's violations of the rules a transcript keeps (browse.md § The transcript's ruler)
    jush.transcript.run(...names)        play the scenes named, or all, each in a new conversation with the scripted model, and print each one's violations; jush.transcript.scenes() lists them
    jush.transcript.planted()            plant a defect of each kind in the running page and print whether its rule reported it
    jush.device(name)                    have the open conversation run on the device name, such as the slot's runner, before its first message; the Cloud's environment is the development setup's, not a device's
    jush.runner({ fresh, backend }) .stop() .start({ backend })   the slot's runner, paired through Add Device; backend: another address to reach the backend at, such as a proxy
    jush.log.console() .network() .sockets() ({ all, modules }), jush.log.mark(name)
    jush.help()                          this text

--headed shows the browser's window.`
