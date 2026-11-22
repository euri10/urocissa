import { beforeEach, expect, it, vi } from 'vitest'
import { createRenderer, createSSRApp, h, nextTick, ssrContextKey, type Component, type ComponentPublicInstance, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import { decideConceptCandidate, requestConceptCandidates } from '@/api/autoTags'
import { useConfigStore } from '@/store/configStore'
import type { AppConfig } from '@/api/config'
import ConceptBuilder from './ConceptBuilder.vue'

vi.mock('@/api/autoTags', () => ({ decideConceptCandidate: vi.fn(), requestConceptCandidates: vi.fn() }))
interface Candidate { label: string, decision: 'pending' | 'confirmed' | 'rejected' | 'skipped' }
interface State {
  candidates: Candidate[]
  loaded: boolean
  custom: string
  error: string
  generate: () => Promise<void>
  decide: (candidate: Candidate, action: 'confirm' | 'reject' | 'clearRejection') => Promise<void>
  addCustom: () => Promise<void>
}
const photo = { id: 'fixture-photo', type: 'image', autoTagsEnabled: true }
const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText: () => undefined, setElementText: () => undefined, patchProp: () => undefined,
  parentNode: () => null, nextSibling: () => null, insert: () => undefined, remove: () => undefined
})

beforeEach(() => {
  setActivePinia(createPinia())
  useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: false } as AppConfig
  vi.resetAllMocks()
  vi.mocked(requestConceptCandidates).mockResolvedValue(['tree', 'water'])
  vi.mocked(decideConceptCandidate).mockResolvedValue(true)
})

function mount() {
  let state: State | undefined
  const subject: Component = { ...ConceptBuilder, render: () => null, created(this: ComponentPublicInstance) {
    state = (this.$ as unknown as { setupState: State }).setupState
  } }
  const app = renderer.createApp(subject, { photo, isolationId: 'detailId' })
  app.provide(ssrContextKey, { modules: new Set<string>() })
  app.mount({})
  if (state === undefined) throw new Error('Concept builder did not mount')
  return { app, state }
}

it('requests suggestions only on demand and saves only explicit decisions', async () => {
  const { app, state } = mount()
  expect(requestConceptCandidates).not.toHaveBeenCalled()
  await state.generate()
  expect(state.candidates).toEqual([{ label: 'tree', decision: 'pending' }, { label: 'water', decision: 'pending' }])
  const first = state.candidates[0]
  if (first === undefined) throw new Error('Missing candidate')
  await state.decide(first, 'confirm')
  expect(first.decision).toBe('confirmed')
  expect(decideConceptCandidate).toHaveBeenCalledWith('fixture-photo', 'tree', 'confirm', 'detailId')
  state.custom = 'rhubarb'
  await state.addCustom()
  expect(state.candidates.at(-1)).toEqual({ label: 'rhubarb', decision: 'confirmed' })
  expect(decideConceptCandidate).toHaveBeenCalledTimes(2)
  app.unmount()
})

it('keeps uncertain failures unconfirmed and does not mutate when read-only', async () => {
  const { app, state } = mount()
  await state.generate()
  const candidate = state.candidates[0]
  if (candidate === undefined) throw new Error('Missing candidate')
  vi.mocked(decideConceptCandidate).mockRejectedValueOnce(new Error('offline'))
  await state.decide(candidate, 'reject')
  expect(candidate.decision).toBe('pending')
  expect(state.error).toContain('may have been saved')
  await state.decide(candidate, 'reject')
  expect(decideConceptCandidate).toHaveBeenCalledTimes(1)
  useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: true } as AppConfig
  await nextTick()
  await state.decide(candidate, 'reject')
  expect(decideConceptCandidate).toHaveBeenCalledTimes(1)
  app.unmount()
})

it('undoes only an explicitly rejected candidate', async () => {
  const { app, state } = mount()
  await state.generate()
  const candidate = state.candidates[0]
  if (candidate === undefined) throw new Error('Missing candidate')
  await state.decide(candidate, 'reject')
  expect(candidate.decision).toBe('rejected')
  await state.decide(candidate, 'clearRejection')
  expect(candidate.decision).toBe('pending')
  expect(decideConceptCandidate).toHaveBeenLastCalledWith('fixture-photo', 'tree', 'clearRejection', 'detailId')
  app.unmount()
})

it('explains locality, uncertainty and published User-tag visibility', async () => {
  const app = createSSRApp(ConceptBuilder, { photo, isolationId: 'detailId' })
  const stub: FunctionalComponent = (_props, { attrs, slots }) => h('div', attrs, slots.default?.())
  for (const name of ['VListItem', 'VSheet', 'VAlert', 'VBtn', 'VProgressLinear', 'VCardSubtitle', 'VChip', 'VTextField']) app.component(name, stub)
  const html = await renderToString(app)
  expect(html).toContain('local AI worker, are limited to a small vocabulary, and may be wrong')
  expect(html).toContain('may be visible on published photos')
  expect(html).toContain('Skipping does not save feedback')
})
