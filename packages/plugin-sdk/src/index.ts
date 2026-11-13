/**
 * The plugin page API: everything a plugin page may use of the web app.
 *
 * The web app is a shell that plugins fill, and a page is written only against
 * this package. The shell therefore knows no plugin, and a plugin knows no part
 * of the shell beyond this package. A page imports this package,
 * `@jush-ai/utils`, Vue, Zod and its own generated types, and nothing else of
 * the workspace.
 *
 * ```text
 * web (services over the backend)      web-gallery (fixture services)
 *            \                              /
 *             +--> generated registry <----+
 *                        |
 *          plugin-browser  plugin-skills  plugin-changes  ...  (feature UI)
 *                        |
 *                   plugin-sdk   definePage, usePage, intents, services, kit
 *                        |
 *                     web-ui     the shell's components and the primitives
 * ```
 *
 * A page is a plain object, `definePage`, that names its plugin and says what
 * it contributes. Everything else about it derives from the plugin's manifest
 * (`crates/plugin-interface/src/manifest.rs`): its plugin's id, imported from
 * its generated module rather than spelled, its types, and its place in the
 * registry. Its fields are fixed: a page declares what it fills, not what it
 * does at a given moment. A slot joins the SDK when a feature needs it, not
 * before.
 *
 * The kit's components are chosen as a set, not by what one page happened to
 * use. A primitive joins it when a page needs it and another page could use it;
 * one only a single page could use stays in that page.
 *
 * The package's major version is the page API's version:
 *
 * - A change that could break a page written against it raises it, such as a
 *   slot, a part of the page context, an intent or a kit component removed or
 *   changed incompatibly. Adding one does not.
 * - Pages of this repository always build against the SDK beside them.
 *
 * The package is private until pages from outside the repository load, which is
 * open and decided with a TypeScript SDK for plugins. The design keeps it
 * possible: such a page is an ES module built against one SDK major version,
 * with Vue and the SDK supplied by the page around it, and every part of the
 * page context passes data. A page whose code nobody reviewed can therefore run
 * in a sandboxed frame, with `usePage()` over messages to the shell, without
 * changing the page's code.
 */

// The page object and its context.
export {
  PluginCallError,
  type PluginCallAnswer,
  definePage,
  type AnyPluginPage,
  usePage,
  type ConversationFileService,
  type ConversationPlugin,
  type IntentService,
  type KindTab,
  type PageContext,
  type PagePlugin,
  type PageDevice,
  type PageWorkspace,
  type TabKind,
  type WindowKind,
  type HistoryDirection,
  type TabUpdate,
  type PluginCallOptions,
  type PluginPage,
  type PluginSettingsSection,
  type PluginState,
} from '@jush-ai/web-ui/plugins/page'
export type { IntentName, IntentPayloads, IntentRequest } from '@jush-ai/web-ui/plugins/intents'
// Where a pinned tab is, as the page's address holds it.
export { definePlace, type TabPlace, type PlaceFields } from '@jush-ai/web-ui/plugins/place'
export { windowHeight } from '@jush-ai/web-ui/ui/window-height'
// What a page keeps live while its plugin is on, apart from the work:
// its stores, which its page
// context's `store` reaches, its sessions by key that outlive the views
// watching them, and its disk cache.
export { definePluginStore, type PluginStore } from '@jush-ai/web-ui/plugins/stores'
export { KEPT_SESSIONS, KeptSessions, type KeptSession, type SessionWatch } from '@jush-ai/web-ui/plugins/kept'
export { PLUGIN_CACHE_BYTES, type PluginCache } from '@jush-ai/web-ui/plugins/cache'
export type {
  OpenUserStream,
  StreamBytes,
  UserStream,
  UserStreamHandlers,
} from '@jush-ai/web-ui/plugins/streams'
export type { DownloadProgress, DeviceArtifact, DeviceDownload } from '@jush-ai/web-ui/devices/installed'
export { downloadProgress } from '@jush-ai/web-ui/devices/installed'
export type { OverlayStore } from '@jush-ai/web-ui/overlay/overlayStore'
// What the page says of a failure, in the page's language.
export { errorMessage } from '@jush-ai/web-ui/infra/errors'
// A copy button's copy: Copied for a moment after, and the app's toast when it fails.
export { useCopy } from '@jush-ai/web-ui/composables/useCopy'
export type { SettingsRowStatus } from '@jush-ai/web-ui/settings/types'
// The style a plugin's own prop of UI text declares (the gallery's Writing page).
export type { HeadlineText, PlaceholderText, SentenceText, TitleText } from '@jush-ai/web-ui/ui/ui-text'

// The plugin's own text in every language Jush speaks:
// `definePluginText(PLUGIN, { en, ... })` in its `src/locales/text.ts`.
export { definePluginText, type Catalogue, type Text } from '@jush-ai/web-ui/i18n/text'
export type { Language } from '@jush-ai/web-ui/i18n/languages'

// The plugin kit. Streams: the liveness a stream's protocol uses.
export {
  pageReturned,
  pageVisibility,
  waitToReconnect,
  watchSilence,
  type ReconnectWait,
  type SilenceWatch,
} from '@jush-ai/web-ui/transport/liveness'
// The live view (`crates/plugin-mobile/page/src/index.ts`): its frames, the decoder that
// shows its pictures, and the viewer's input in the picture's coordinates,
// which every source's view shares.
export * from '@jush-ai/web-ui/live/protocol'
export {
  LiveFrameAssembler,
  LiveFrameSplitter,
  LiveFrameReader,
  decodeAcks,
  encodeAcks,
  encodeControl,
  encodeVideo,
  framed,
  type LiveChunkAck,
  type LiveRawFrame,
  type LiveVideoChunk,
  type LiveVideoFrame,
} from '@jush-ai/web-ui/live/frames'
export {
  CanvasPictures,
  picturesSupported,
  type LivePictureStream,
  type PictureHandlers,
  type PictureSink,
} from '@jush-ai/web-ui/live/pictures'
export {
  ALT,
  CONTROL,
  ClickCount,
  META,
  SHIFT,
  keyInput,
  modifiers,
  pointerInput,
  touchInput,
  wheelInput,
  type KeyInput,
  type ModifierState,
  type PointerInput,
  type WheelInput,
} from '@jush-ai/web-ui/live/input'
export {
  LiveLink,
  REFUSED_FRAME,
  SILENT_STREAM,
  UNANSWERED_STREAM,
  type LiveConnection,
  type LiveFrames,
  type LiveLinkHandlers,
  type LiveLinkState,
} from '@jush-ai/web-ui/live/link'
export {
  DRAWN_FRAME_ROOM,
  SQUARE_CORNERS,
  drawnFrame,
  observeSettledSize,
  scaledCorners,
  drawnFramePictureClip,
  screenClip,
  screenPath,
  type CornerPoint,
  type FrameBox,
  type FrameButton,
  type DrawnFrameShape,
  type ScreenCorner,
  type ScreenCorners,
} from '@jush-ai/web-ui/live/drawn-frame'
export { default as LiveDrawnFrame } from '@jush-ai/web-ui/live/LiveDrawnFrame.vue'
export { default as LiveActivityOverlay } from '@jush-ai/web-ui/live/LiveActivityOverlay.vue'
// Whether the person asked for less motion, as the system's Reduce Motion does.
export { prefersReducedMotion } from '@jush-ai/web-ui/ui/stream-reveal'
// Composables.
// A field that takes the focus as its surface opens, as a phone allows.
export { useAutofocus } from '@jush-ai/web-ui/ui/autofocus'
export {
  vContextMenu,
  type ContextMenuDeclaration,
  type ContextMenuEntry,
  type ContextMenuGroup,
  type ContextMenuValue,
} from '@jush-ai/web-ui/ui/context-menu'

// Files: the shapes the conversation files service gives, and their paths.
export {
  emptyChangeSet,
  type ChangeFile,
  type ChangeMode,
  type ChangeSetSource,
  type ChangeSources,
  type ReadCallChange,
} from '@jush-ai/web-ui/files/changes'
// A request's changes: what the `edit` intent names, and the files the Change view lists.
export {
  requestEditSelectionSchema,
  sameEditRef,
  sameRequestFiles,
  type RequestChangeSource,
  type RequestEdit,
  type RequestEditRef,
  type RequestEditSelection,
  type RequestFile,
  type TranscriptRequest,
} from '@jush-ai/web-ui/files/request-changes'
// The repositories Commits offers for a conversation's directory.
export type { Repositories, RepositoryEntry } from '@jush-ai/web-ui/files/repositories'
export type { FileBrowserSource } from '@jush-ai/web-ui/files/types'
export { baseName, isAbsoluteDevicePath, joinPath, relativePath } from '@jush-ai/web-ui/files/paths'
export { downloadUrl } from '@jush-ai/web-ui/files/download'
export { formatByteRatio, formatBytes } from '@jush-ai/web-ui/files/format'
export { treeLayout } from '@jush-ai/web-ui/files/tree-layout'
// A window's view says whether it has read what it first needs, and its empty area waits for it.
export { usePaneReady, usePaneWaiting } from '@jush-ai/web-ui/agent/pane-ready'
export { default as PaneReading } from '@jush-ai/web-ui/ui/PaneReading.vue'

// Components: settings, controls, progress, navigation, files and icons.
export { default as AddressBar } from '@jush-ai/web-ui/agent/AddressBar.vue'
export { default as Button } from '@jush-ai/web-ui/ui/Button.vue'
export type { Action, Operation } from '@jush-ai/web-ui/ui/operations'
export { default as ChangeView } from '@jush-ai/web-ui/files/ChangeView.vue'
export { HTML_SANDBOX } from '@jush-ai/web-ui/files/preview'
export { default as ConfirmDialog } from '@jush-ai/web-ui/ui/ConfirmDialog.vue'
export { default as Dialog } from '@jush-ai/web-ui/ui/Dialog.vue'
export { default as Dropdown } from '@jush-ai/web-ui/ui/Dropdown.vue'
export { default as ExternalLink } from '@jush-ai/web-ui/ui/ExternalLink.vue'
export { default as Fold } from '@jush-ai/web-ui/ui/Fold.vue'
export { default as FileIcon } from '@jush-ai/web-ui/files/FileIcon.vue'
export { default as FileView } from '@jush-ai/web-ui/files/FileView.vue'
export { default as AttachmentView } from '@jush-ai/web-ui/files/AttachmentView.vue'
export { default as MediaView } from '@jush-ai/web-ui/files/MediaView.vue'
export { default as LineCounts } from '@jush-ai/web-ui/files/LineCounts.vue'
export { default as FoldChevron } from '@jush-ai/web-ui/ui/FoldChevron.vue'
export { default as IconButton } from '@jush-ai/web-ui/ui/IconButton.vue'
export { default as InlineError } from '@jush-ai/web-ui/ui/InlineError.vue'
export { default as IndeterminateSpinner } from '@jush-ai/web-ui/ui/IndeterminateSpinner.vue'
export { default as Menu } from '@jush-ai/web-ui/ui/Menu.vue'
export { default as MenuDivider } from '@jush-ai/web-ui/ui/MenuDivider.vue'
export { default as MenuGroup } from '@jush-ai/web-ui/ui/MenuGroup.vue'
export { default as MenuItem } from '@jush-ai/web-ui/ui/MenuItem.vue'
export { default as Popover } from '@jush-ai/web-ui/ui/Popover.vue'
export { default as ProgressLine } from '@jush-ai/web-ui/ui/ProgressLine.vue'
export { default as RegionStatus } from '@jush-ai/web-ui/ui/RegionStatus.vue'
export { default as ScrollArea } from '@jush-ai/web-ui/ui/ScrollArea.vue'
export { default as SettingsGroup } from '@jush-ai/web-ui/settings/SettingsGroup.vue'
export { default as SettingsPage } from '@jush-ai/web-ui/settings/SettingsPage.vue'
export { default as SettingsRow } from '@jush-ai/web-ui/settings/SettingsRow.vue'
export { default as Slider } from '@jush-ai/web-ui/ui/Slider.vue'
export { default as Switch } from '@jush-ai/web-ui/ui/Switch.vue'
export { default as TextInput } from '@jush-ai/web-ui/ui/TextInput.vue'
export { default as Toolbar } from '@jush-ai/web-ui/ui/Toolbar.vue'
export type { ToolbarAction } from '@jush-ai/web-ui/ui/Toolbar.vue'
export { default as Tooltip } from '@jush-ai/web-ui/ui/Tooltip.vue'
export { ICON_PX } from '@jush-ai/web-ui/ui/icon-metrics'
// A system's logo, as a device's mark draws it.
export { default as SystemLogo } from '@jush-ai/web-ui/devices/DeviceLogo.vue'
// A device as every menu row and list draws it: its mark with its state, and its state's words.
export { deviceGlyph } from '@jush-ai/web-ui/devices/device-glyphs'
export { deviceStatusLabel, type DeviceState } from '@jush-ai/web-ui/devices/state'
export type { DeviceMark } from '@jush-ai/web-ui/devices/mark'
