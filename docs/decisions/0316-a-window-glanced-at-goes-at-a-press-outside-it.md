---
status: proposed
---

# A window glanced at goes at a press outside it

Supersedes: 0308 (a conversation's work opens in windows on a taskbar), the
part where a floating, anchored or maximized window stays until the user puts
it away.

## Problem

Alex opens `cookie.ts` from a reply to check one line. The file's window
floats over the conversation. To go back to the conversation, Alex has to find
the window's Minimize button or press Escape; a click on the conversation
leaves the window over it. Opening a file, a change or a picture is most often
a glance, and every glance ends with a trip to the window's corner.

## Decision

**A press outside a window minimizes it, unless it is kept open.**

- A press with the primary button anywhere on the page outside the
  conversation's windows minimizes every window that floats, is anchored or
  is maximized, and is not kept open. The press still does what it does
  there: a press in the composer minimizes the windows and puts the caret in
  the message; a press on a conversation in the sidebar minimizes them and
  opens it.
- A press in a window, on the taskbar, on a control marked to keep the
  windows (`KEEPS_WINDOWS`: an agent's link, a picture's thumbnail, a file's
  or a commit's link in a reply, which open or change a window, and the
  column's divider and the gaps between its rows, which size windows), or
  while a menu, a dialog or the media viewer is open, minimizes nothing; a
  press while a menu is open closes the menu first, as it does today.
- A docked window never goes at a press outside: the column is a layout the
  user arranged.
- A maximized window that goes comes back maximized from the taskbar, as a
  system's minimized maximized window does.
- Escape stays as it is: it puts away the window in front.

**Keep Open** is a button in a floating, anchored or maximized window's title
bar, before Minimize, with a pin. Pressed, the window stays at a press outside
it, until the button is pressed again. A docked window shows no Keep Open.
The window keeps the choice wherever it goes and when it is closed and opened
again, as it keeps its box.

```text
 +------------------------------------------------------------+
 | [tab] [tab]                  ...  |  [pin] [-] [max] [x]   |
 +------------------------------------------------------------+
                                        ^ Keep Open
```

The name is Keep Open, not Pin: the taskbar's Pin to Taskbar is another
concept.

### Build

- `StageWindow` gains `keptOpen?: boolean`, saved with the layout and with a
  closed window; an older layout has none, which is not kept. It gains
  `wasMaximized?: boolean` too, set when a maximized window is minimized,
  which `restore` maximizes again.
- `WindowStage` listens for `pointerdown` on the document, in the capture
  phase, on a computer only; a phone keeps its own rule, where a tap beside
  the one window puts it away. A press minimizes the windows the rule names
  through the model's `minimizeUnkept`.
- Where several areas share a page, as the gallery's specimens do, the
  place that holds one bounds it (`WINDOW_SCOPE`): a press outside it is
  not that area's.
- The work window's title bar gains the button; its text is in the nine
  locales.

## Prior art

macOS Quick Look and a popover close at a click outside them; Windows Peek
hides at a move away. A window that should stay is kept by a pin in Visual
Studio's tool windows (Auto Hide off) and in the Windows Sticky Notes.

## Alternatives

- **Minimize only the window in front**, as Escape does: the press that goes
  back to the conversation would leave the other windows over it.
- **Count a docked window too**: a press to scroll or select the conversation
  would empty the column.
