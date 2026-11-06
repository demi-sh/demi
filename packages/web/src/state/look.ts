import { watch, type Ref } from 'vue'
import { applyProductAppearance, applyTranscriptTextSize, productAppearance } from '@demicodes/web-ui/theme/productAppearance'
import { applyThemeToDocument, setThemeChoice } from '@demicodes/web-ui/theme/appTheme'
import { setPageLanguage } from '@demicodes/web-ui/i18n/text'
import { keepShownLanguage, resolveLanguage } from '@demicodes/web-ui/i18n/languages'
import type { useProduct } from './product'
import type { usePreferences } from './preferences'
import type { useResources } from './resources'

/**
 * Puts the user's appearance on the document, the web app's or a program's
 * page that shows Demi's elements: the product's axes, then the tone, the
 * accent, the transcript's size and the theme as they change. Answers what
 * stops it.
 */
export function followAppearance(resources: ReturnType<typeof useResources>): () => void {
  for (const [axis, value] of Object.entries(productAppearance)) {
    document.documentElement.setAttribute(`data-${axis}`, value)
  }
  // Each value is put on the document when it changes, not each time the
  // product's state does: the appearance is made anew for every message of
  // the sync channel, and an attribute written again, even unchanged, tells
  // what watches the document's appearance to draw again.
  const stopAppearance = watch(
    [
      () => resources.appearance.tone,
      () => resources.appearance.accent,
      () => resources.appearance.fontSize,
      () => resources.appearance.theme,
    ],
    ([tone, accent, fontSize, theme]) => {
      applyProductAppearance({ tone, accent })
      applyTranscriptTextSize(fontSize)
      setThemeChoice(theme)
    },
    { immediate: true },
  )
  const stopTheme = applyThemeToDocument()
  return () => {
    stopAppearance()
    stopTheme()
  }
}

/**
 * The page speaks the language the user chose, or with System the first of
 * `languages`, the browser's, that Demi speaks; a change of either, the
 * choice made on another page included, applies at once. Until the
 * preferences arrive the page stays in the language it started in.
 */
export function followLanguage(
  product: ReturnType<typeof useProduct>,
  preferences: ReturnType<typeof usePreferences>,
  languages: Ref<readonly string[]>,
): () => void {
  return watch(
    () => (product.snapshot ? resolveLanguage(preferences.language, languages.value) : null),
    (language) => {
      if (language !== null) {
        setPageLanguage(language)
        keepShownLanguage(language)
      }
    },
    { immediate: true },
  )
}
