// `jush.model` (browse.md § The scripted model): the model a check scripts.
//
// `jush.model.answers([...])` queues the model's replies to the next requests
// of a turn, and makes sure the slot's backend has the model, as a provider
// entry named Scripted, and that the conversations the page starts use it.
// `jush.model.requests()` answers what the backend asked since.
import type { Page } from 'playwright'
import { SCRIPTED_MODEL, type Answer, type Asked } from '../model'
import { Failure, webBase, type Tool } from '../tool'

/** The provider entry's name in Settings and in the model menu. */
const LABEL = 'Scripted'

export function model(tool: Tool) {
  /** The id of the backend's entry for the scripted model, created when it has none. */
  async function entry(page: Page): Promise<string> {
    const base = webBase(tool.slot)
    const url = tool.model().url
    const listed = await page.request.get(`${base}/api/providers`)
    if (!listed.ok()) {
      throw new Failure(`The backend answered ${listed.status()} for the provider entries; jush.up() starts it and signs in`)
    }
    const { providers } = await listed.json() as { providers: { id: string, label: string, baseUrl: string | null }[] }
    const known = providers.find((provider) => provider.baseUrl === url)
    if (known) {
      return known.id
    }
    const created = await page.request.post(`${base}/api/providers`, {
      data: {
        source: 'compatible',
        providerType: 'anthropic',
        label: LABEL,
        // The endpoint reads no key; the entry needs one.
        apiKey: 'sk-ant-scripted',
        baseUrl: url,
        models: [{
          id: SCRIPTED_MODEL,
          displayName: LABEL,
          contextWindow: 200_000,
          outputLimit: null,
          thinkingEfforts: [],
          acceptedExtensions: null,
          fastTier: null,
        }],
      },
    })
    if (!created.ok()) {
      throw new Failure(`The backend answered ${created.status()} for the scripted model's entry: ${await created.text()}`)
    }
    const { provider } = await created.json() as { provider: { id: string } }
    return provider.id
  }

  /**
   * Has the composer the page shows use the scripted model, chosen through its
   * model menu as a person chooses it; the composer keeps it for the
   * conversations started after.
   */
  async function choose(page: Page, id: string): Promise<void> {
    const wanted = `${id}:${SCRIPTED_MODEL}`
    // The chip by its hook (`ModelSelector`), in any language. Its button
    // comes once the page has the models; the chip names the model it shows,
    // and none while the one the composer remembers is not available.
    const chip = page.locator('[data-model-selector]').filter({ visible: true }).first()
    await chip.getByRole('button').waitFor()
    if (await chip.getAttribute('data-model') === wanted) {
      return
    }
    await chip.getByRole('button').click()
    await page.locator('[data-model-menu-models]').hover()
    // The model's row in the menu; the chip carries the same hook and stands outside it.
    await page.locator(`[role=menu] [data-model=${JSON.stringify(wanted)}]`).click()
    await page.waitForFunction((model) => document.querySelector(`[data-model-selector][data-model=${JSON.stringify(model)}]`) !== null, wanted).catch(() => {
      throw new Failure('The composer does not show the scripted model: its menu did not take the choice')
    })
    // A choice leaves the menu open, as a setting's does: one Escape closes the submenu of models, the next the menu.
    for (const _level of ['submenu', 'menu']) {
      if (await page.locator('[role=menu]').count() > 0) {
        await page.keyboard.press('Escape')
      }
    }
  }

  return {
    /**
     * Queues `answers`, each the model's reply to one request of a turn, in
     * place of what was queued, and has the composer the page shows use the
     * model; answers the entry's id.
     */
    answers: async (answers: Answer[]): Promise<string> => {
      const page = await tool.browser.page()
      const id = await entry(page)
      await choose(page, id)
      tool.model().answers(answers)
      return id
    },
    /** What the backend asked the model since it listened, or since `forget`. */
    requests: (): Asked[] => tool.model().requests(),
    /** How many queued answers no request has taken. */
    left: (): number => tool.model().left,
    /** Drops the queue and the requests. */
    forget: (): void => tool.model().forget(),
  }
}
