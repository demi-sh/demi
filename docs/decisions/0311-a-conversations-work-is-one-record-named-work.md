---
status: accepted
---

# A conversation's work is one record, named work

Supersedes: 0014 (the work panel has addresses), the `panel` field and the
selection each browser keeps; 0308 (a conversation's work opens in windows on
a taskbar), where it leaves which window is in front to each browser.

## Problem

Alex opens `cookie.ts` in a conversation on a desktop, then minimizes the
Files window on a laptop. Back on the desktop the next morning, Alex clicks
the conversation in the sidebar: Files comes up in front again, and the
desktop writes that layout back, so the laptop shows it too.

Since 0308 the layout of the windows is the conversation's, kept by the
backend and the same on every device, but two facts it depends on are still
each browser's own, kept from the panel the windows replaced:

- Whether a tab's window is in front: `workPanelOpen`, per browser. A browser
  that last saw a window in front shows it again, whatever the layout says.
- Which tab each window shows: derived from `workPanelHistory`, the tabs this
  browser selected, newest last. Another device shows another tab in the same
  window.

Reproduced on the running product: the browser's own state brought a window
the layout held minimized to the front and posted the layout back.

The name has the same history. The record of a conversation's tabs and
windows is called the panel throughout: `WorkPanel`, `PanelTab`, `PanelKind`,
`conversation_panels`, the `/panel/changes` route, the address's `panel`
field, and about 300 files besides, though no panel exists any more. Several
interfaces still describe the panel's behavior, such as `panel.select`
"opens the panel" and `shows` "the panel opens and selects it", which now
mean a window coming to the front.

## Decision

**A conversation's work is one record, its tabs and its windows, which the
backend keeps and every device shows alike; it is called work everywhere.**

### One owner

- The work's layout holds, besides each window's place, the window in front
  and the tabs in the order they were last shown, newest last, `recent`:
  each window shows its newest. A browser keeps none of it: `workPanelOpen`
  and `workPanelHistory` go. The page shows its own layout as it writes it,
  and takes the backend's when another device wrote a newer one.
- An address that names no tab, such as the sidebar's, names nothing: the
  windows stand as the layout says, and the address then names the tab of
  the one in front.
- Opening a conversation shows its work as its layout says. An address that
  names a tab brings that tab's window to the front, which changes the
  layout, as a press would.
- Closing the shown tab of a window shows the one it showed before, as
  today and as VS Code does; with none, the window closes.
- A phone writes no layout, as 0308 says: what it shows in front and the
  tabs it showed are its own, for the page's lifetime, shown over the
  layout's order, so what a computer writes meanwhile still counts. A
  computer writes the tabs it showed in a conversation it has not opened,
  as a tab an agent asked to show there, once it opens it, over the layout
  it then reads.
- What each browser applied of a tab's `shows` count stays the browser's: it
  says which requests to show a tab this page acted on, not where the work
  stands.

### One name

| Was | Becomes |
| --- | --- |
| `WorkPanel`, module `panel` (web API protocol) | `Work`, module `work` |
| a conversation's entry's `panel`, its metadata's `panelStates` | `work`, `tabStates` |
| `PanelTab`, `CreatePanelTab`, `PanelChange(s)`, `PanelAnswer`, `PanelDocument`, `PanelEffect` | `WorkTab`, `CreateWorkTab`, `WorkChange(s)`, `WorkAnswer`, `WorkDocument`, `WorkEffect` |
| `PANEL_TABS_MAX`, `PANEL_BYTES_MAX`, `PANEL_WINDOWS_BYTES_MAX` | `WORK_TABS_MAX`, `WORK_BYTES_MAX`, `WORK_LAYOUT_BYTES_MAX` |
| table `conversation_panels` | `conversation_work`, by a migration |
| `POST /conversations/{id}/panel/changes` | `POST /conversations/{id}/work/changes` |
| error codes `panel_full`, `unknown_panel_kind` | `work_full`, `unknown_tab_kind` |
| `Manifest::panel_kinds`, `Page::panel_kind` | `tab_kinds`, `tab_kind` |
| `PluginPort::panel_tabs`, `create_panel_tab`, `update_panel_tab`, `remove_panel_tab` | `work_tabs`, `create_tab`, `update_tab`, `remove_tab` |
| `Request::PanelTab` (the user closed a tab) | `Request::TabClosed` |
| SDK `PanelKind`, `PanelWindow`, `PageHost.panel`, `PageContext.panel` | `TabKind`, `WindowKind`, `PageHost.work`, `PageContext.work` |
| web-ui `PanelTabKind`, `PanelState`, `ShownPanel`, `PanelTabs`, `PanelAddress`, files `panel-*.ts`, `panel-kinds/` | `BoundTabKind`, `TabsState`, `ShownWork`, `WorkTabs`, `WorkAddress`, `work-*.ts`, `tab-kinds/`; the resolved tab a window shows, `WorkTab` before, is `ShownTab` |
| web `useWorkPanel`, `workPanelShown` | `useWork`, `tabsShown` |
| the address's `?panel=t7` | `?tab=t7` |
| plugins' `panel.rs` modules; `PanelReason`, `PanelButton`, `PanelRecord` of the mobile and browser pages | `tabs.rs`; `ControlReason`, `ControlButton`, `ControlRecord` |

"Panel" stays where it names a surface of its own, such as a popover's
panel or a live view's corners.

### What each interface means now

- `work.select(conversation, kind, id)`: brings the window of the tab `id`
  to the front on it, once the work has the tab.
- `TabKind.shows`: a rise of the count brings the tab's window to the front
  on it.
- `work.canGo` and `work.go`: whether the web browser's Back or Forward stays
  in the conversation's windows, and the step.
- `TabKind.pinned`: the kind is its window's one view, such as Files, there
  in every conversation and never saved.
- `create_tab`'s `index`: the tab's place among its window's tabs.
- `jush browser show` and every other command that shows a tab: its window
  comes to the front, as their descriptions already say.

### Older data

- The migration renames the table; its rows, tabs and layouts stay.
- A layout without the window in front or `recent` shows no window in front
  and each window on its first tab.
- An address with `panel=` is read once as `tab=` and rewritten, so a link
  kept before still opens its tab.
- A browser's `workPanelOpen` and `workPanelHistory` are dropped as its
  local state is next read; its `workPanelShown`, which tab showings it
  already applied, is read as `tabsShown`, so a tab is not shown again.

## Spike

The reproduction above, on the product with the stale state of a second
browser put in place: the window came to the front and two
`/panel/changes` requests followed. Nothing else was uncertain.

## Prior art

- VS Code keeps a workspace's editor layout in one serialized state: the
  grid, the groups in the order they were last active
  (`mostRecentActiveGroups` in
  `src/vs/workbench/browser/parts/editor/editorPart.ts`), and each group's
  editors with their most recently used order (`mru` of
  `ISerializedEditorGroupModel` in
  `src/vs/workbench/common/editor/editorGroupModel.ts`). Closing the active
  editor shows the most recent one (`workbench.editor.focusRecentEditorAfterClose`).
  Jush keeps the same, for one window in front rather than a recent order of
  windows, which its taskbar's order stands for.

## Alternatives

- **Keep each browser's selection, and sync only the layout.** Two devices
  then disagree on what a window shows, the case above.
- **Keep the name panel, and redefine it.** Cheaper, but the name then means
  something no one sees, and every new interface would be named after it.
- **Show the tab to the right of a closed one, as Chrome does.** It needs no
  order stored, but changes what closing a tab shows today.

## Open questions

None.
