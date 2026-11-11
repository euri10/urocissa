import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createRenderer, createSSRApp, h, nextTick, ssrContextKey, type Component, type ComponentPublicInstance, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import AutoTagLearning from './AutoTagLearning.vue'
import { getAutoTagStatus, recalculateAutoTags, type AutoTagRecalculation } from '@/api/autoTags'
import { useConfigStore } from '@/store/configStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import type { AppConfig } from '@/api/config'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))
vi.mock('@/api/autoTags', () => ({ getAutoTagStatus: vi.fn(), recalculateAutoTags: vi.fn() }))
vi.mock('@/store/searchFacetStore', () => ({ useSearchFacetStore: () => ({ fetched: true, fetchFacets: vi.fn().mockResolvedValue(undefined) }) }))

const initial: AutoTagRecalculation = { running: false, upToDate: true, lastCompletedAt: 1000, changes: { added: 0, removed: 0 }, lastError: null }
const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText: () => undefined, setElementText: () => undefined, patchProp: () => undefined,
  parentNode: () => null, nextSibling: () => null, insert: () => undefined, remove: () => undefined
})
interface State {
  recalculate: () => Promise<void>
  accept: (next: AutoTagRecalculation) => void
  status: AutoTagRecalculation | null
  busy: boolean
  heading: string
  result: string
  requestError: string
}
const stub: FunctionalComponent = (_props, { attrs, slots }) => h('div', attrs, slots.default?.())

beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  setActivePinia(createPinia())
  useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: false } as AppConfig
  vi.mocked(getAutoTagStatus).mockResolvedValue(initial)
  vi.mocked(recalculateAutoTags).mockResolvedValue({ ...initial, lastCompletedAt: 2000 })
})
afterEach(() => { vi.useRealTimers() })

async function mount() {
  let state: State | undefined
  const subject: Component = { ...AutoTagLearning, render: () => null, created(this: ComponentPublicInstance) {
    state = (this.$ as unknown as { setupState: State }).setupState
  } }
  const app = renderer.createApp(subject)
  app.provide(ssrContextKey, { modules: new Set<string>() })
  app.mount({})
  await nextTick()
  if (!state) throw new Error('Learning panel did not mount')
  return { app, state }
}

it('prevents duplicate requests and refreshes collections on completion', async () => {
  const { app, state } = await mount()
  let finish: ((value: AutoTagRecalculation) => void) | undefined
  vi.mocked(recalculateAutoTags).mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const pending = state.recalculate()
  expect(state.heading).toContain('Recalculating')
  await state.recalculate()
  expect(recalculateAutoTags).toHaveBeenCalledTimes(1)
  finish?.({ ...initial, lastCompletedAt: 2000, changes: { added: 2, removed: 1 } })
  await pending
  expect(state.result).toBe('2 suggestions added · 1 removed.')
  expect(state.heading).toBe('Auto tags up to date')
  expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(1)
  app.unmount()
  expect(vi.getTimerCount()).toBe(0)
})

it('keeps the previous completion on failure and supports an unchanged retry', async () => {
  const { app, state } = await mount()
  vi.mocked(recalculateAutoTags).mockRejectedValueOnce(new Error('offline'))
  await state.recalculate()
  expect(state.status?.lastCompletedAt).toBe(1000)
  expect(state.requestError).toContain('Could not complete')
  expect(state.heading).toBe('Recalculation could not be confirmed')
  expect(state.busy).toBe(false)
  await state.recalculate()
  expect(state.requestError).toBe('')
  expect(state.result).toBe('No suggestions changed.')
  app.unmount()
})

it('does not let an older status response overwrite a completed manual run', async () => {
  let finish: ((value: AutoTagRecalculation) => void) | undefined
  vi.mocked(getAutoTagStatus).mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const { app, state } = await mount()
  await state.recalculate()
  finish?.(initial)
  await nextTick()
  expect(state.status?.lastCompletedAt).toBe(2000)
  expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(1)
  app.unmount()
})

it('refreshes after automatic learning and refuses read-only and visitor writes', async () => {
  const { app, state } = await mount()
  state.accept({ ...initial, lastCompletedAt: 3000 })
  expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(1)
  for (const config of [{ isAdmin: true, readOnlyMode: true }, { isAdmin: false, readOnlyMode: false }]) {
    useConfigStore('mainId').config = config as AppConfig
    await state.recalculate()
  }
  expect(recalculateAutoTags).not.toHaveBeenCalled()
  app.unmount()
})

it.each([true, false])('renders the owner boundary and status text (owner=%s)', async (owner) => {
  useConfigStore('mainId').config = { isAdmin: owner, readOnlyMode: false } as AppConfig
  const subject: Component = { ...AutoTagLearning, created(this: ComponentPublicInstance) {
    const state = (this.$ as unknown as { setupState: State }).setupState
    state.status = initial
  } }
  const app = createSSRApp(subject)
  for (const name of ['VSheet', 'VListItem', 'VListItemTitle', 'VListItemSubtitle', 'VCardSubtitle', 'VBtn']) app.component(name, stub)
  const html = await renderToString(app)
  expect(html.includes('Recalculate Auto tags')).toBe(owner)
  if (owner) {
    expect(html).toContain('Auto tags up to date')
    expect(html).toContain('No suggestions changed.')
    expect(html).toContain('automatically update suggestions')
  }
})
