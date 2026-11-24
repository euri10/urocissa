import { beforeEach, expect, it, vi } from 'vitest'
import { createRenderer, createSSRApp, defineComponent, h, nextTick, provide, ref, ssrContextKey, type Component, type ComponentPublicInstance, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import { decideConceptCandidate, requestConceptCandidates } from '@/api/autoTags'
import { useConfigStore } from '@/store/configStore'
import { useDataStore } from '@/store/dataStore'
import { useResolvedRouteResource } from '@/script/hook/useRouteResource'
import type { AppConfig } from '@/api/config'
import { BackendDataParser } from '@/type/schemas'
import ConceptBuilder from './ConceptBuilder.vue'
import { createConceptReviewState, conceptReviewKey } from './conceptReviewState'

vi.mock('@/api/autoTags', () => ({ decideConceptCandidate: vi.fn(), requestConceptCandidates: vi.fn() }))
vi.mock('@/api/fetchRouteResource', () => ({ fetchRouteResource: vi.fn() }))
vi.mock('@/store/workerStore', () => ({ useWorkerStore: () => ({ worker: null, imgWorker: [], initializeWorker: vi.fn(), terminateWorker: vi.fn() }) }))
interface Candidate { label: string, decision: 'pending' | 'confirmed' | 'rejected' | 'skipped' }
interface State {
  candidates: Candidate[]
  loaded: boolean
  busy: boolean
  custom: string
  error: string
  generate: () => Promise<void>
  decide: (candidate: Candidate, action: 'confirm' | 'reject' | 'clearRejection') => Promise<void>
  addCustom: (action?: 'confirm' | 'reject') => Promise<void>
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
  vi.mocked(requestConceptCandidates).mockResolvedValue([
    { label: 'tree', decision: 'pending' }, { label: 'water', decision: 'pending' }
  ])
  vi.mocked(decideConceptCandidate).mockResolvedValue(true)
})

function mount(review = createConceptReviewState(), selectedPhoto = photo) {
  let state: State | undefined
  const subject: Component = { ...ConceptBuilder, render: () => null, created(this: ComponentPublicInstance) {
    state = (this.$ as unknown as { setupState: State }).setupState
  } }
  const app = renderer.createApp(subject, { photo: selectedPhoto, isolationId: 'detailId' })
  app.provide(conceptReviewKey, review)
  app.provide(ssrContextKey, { modules: new Set<string>() })
  app.mount({})
  if (state === undefined) throw new Error('Concept builder did not mount')
  return { app, state }
}

it('retains the review across viewer remounts and restores saved decisions', async () => {
  const review = createConceptReviewState()
  const first = mount(review)
  await first.state.generate()
  const candidate = first.state.candidates[0]
  if (candidate === undefined) throw new Error('Missing candidate')
  await first.state.decide(candidate, 'confirm')
  first.app.unmount()
  const second = mount(review)
  expect(second.state.candidates).toEqual([
    { label: 'tree', decision: 'confirmed' }, { label: 'water', decision: 'pending' }
  ])
  expect(requestConceptCandidates).toHaveBeenCalledTimes(1)
  vi.mocked(requestConceptCandidates).mockResolvedValueOnce([
    { label: 'tree', decision: 'confirmed' }, { label: 'water', decision: 'rejected' }
  ])
  await second.state.generate()
  expect(second.state.candidates).toEqual([
    { label: 'tree', decision: 'confirmed' }, { label: 'water', decision: 'rejected' }
  ])
  second.app.unmount()
  const other = mount(review, { ...photo, id: 'another-photo' })
  expect(other.state.candidates).toEqual([])
  expect(other.state.loaded).toBe(false)
  expect(requestConceptCandidates).toHaveBeenCalledTimes(2)
  other.app.unmount()
})

it('retains decisions when collection refresh hands the viewer to its direct snapshot and back', async () => {
  const main = useDataStore('mainId')
  const detail = useDataStore('detailId')
  const fixture = { ...BackendDataParser.parse({ ...photo, pending: false, width: 10, height: 10, size: 10, ext: 'jpg', tags: [] }), thumbhashUrl: null, timestamp: 1 }
  main.data.set(7, fixture)
  main.hashMapData.set(photo.id, 7)
  detail.data.set(0, fixture)
  detail.hashMapData.set(photo.id, 0)
  let current: State | undefined
  const subject: Component = { ...ConceptBuilder, render: () => null, created(this: ComponentPublicInstance) {
    current = (this.$ as unknown as { setupState: State }).setupState
  } }
  const app = renderer.createApp(defineComponent({
    setup() {
      provide(conceptReviewKey, createConceptReviewState())
      const { resource } = useResolvedRouteResource(ref(photo.id), 'mainId', 'detailId')
      return () => resource.value === undefined ? null : h(subject, {
        key: `${resource.value.isolationId}:${photo.id}`,
        photo: resource.value.data,
        isolationId: resource.value.isolationId
      })
    }
  }))
  app.provide(ssrContextKey, { modules: new Set<string>() })
  app.mount({})
  if (current === undefined) throw new Error('Missing initial review')
  await current.generate()
  const candidate = current.candidates[0]
  if (candidate === undefined) throw new Error('Missing candidate')
  vi.mocked(decideConceptCandidate).mockImplementationOnce(async () => {
    main.clearAll()
    await nextTick()
    return true
  })
  await current.decide(candidate, 'confirm')
  expect(current.candidates).toEqual([
    { label: 'tree', decision: 'confirmed' }, { label: 'water', decision: 'pending' }
  ])
  main.data.set(7, fixture)
  main.hashMapData.set(photo.id, 7)
  await nextTick()
  expect(current.candidates).toEqual([
    { label: 'tree', decision: 'confirmed' }, { label: 'water', decision: 'pending' }
  ])
  expect(requestConceptCandidates).toHaveBeenCalledTimes(1)
  current.custom = 'rhubarb'
  vi.mocked(decideConceptCandidate).mockImplementationOnce(async () => {
    main.clearAll()
    await nextTick()
    return true
  })
  await current.addCustom()
  expect(current.candidates).toEqual([
    { label: 'tree', decision: 'confirmed' }, { label: 'water', decision: 'pending' },
    { label: 'rhubarb', decision: 'confirmed' }
  ])
  expect(requestConceptCandidates).toHaveBeenCalledTimes(1)
  app.unmount()
})

it('ignores an old decision after navigating away and back to the same photo', async () => {
  const review = createConceptReviewState()
  const first = mount(review)
  await first.state.generate()
  const oldCandidate = first.state.candidates[0]
  if (oldCandidate === undefined) throw new Error('Missing candidate')
  let finishOld: (value: boolean) => void = () => { throw new Error('Missing old request') }
  let finishNew: (value: boolean) => void = () => { throw new Error('Missing new request') }
  vi.mocked(decideConceptCandidate).mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve }))
  const oldRequest = first.state.decide(oldCandidate, 'confirm')
  first.app.unmount()
  const other = mount(review, { ...photo, id: 'another-photo' })
  other.app.unmount()
  const again = mount(review)
  await again.state.generate()
  const newCandidate = again.state.candidates[0]
  if (newCandidate === undefined) throw new Error('Missing new candidate')
  vi.mocked(decideConceptCandidate).mockImplementationOnce(() => new Promise(resolve => { finishNew = resolve }))
  const newRequest = again.state.decide(newCandidate, 'reject')
  finishOld(true)
  await oldRequest
  expect(again.state.busy).toBe(true)
  expect(newCandidate.decision).toBe('pending')
  finishNew(true)
  await newRequest
  expect(newCandidate.decision).toBe('rejected')
  again.app.unmount()
})

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

it('teaches an owner-entered negative without requiring model suggestions and allows Undo', async () => {
  const { app, state } = mount()
  state.custom = ' bridge '
  await state.addCustom('reject')
  expect(requestConceptCandidates).not.toHaveBeenCalled()
  expect(decideConceptCandidate).toHaveBeenCalledExactlyOnceWith('fixture-photo', 'bridge', 'reject', 'detailId')
  expect(state.candidates).toEqual([{ label: 'bridge', decision: 'rejected' }])
  expect(state.loaded).toBe(true)
  expect(state.custom).toBe('')
  const candidate = state.candidates[0]
  if (candidate === undefined) throw new Error('Missing explicit negative')
  await state.decide(candidate, 'clearRejection')
  expect(candidate.decision).toBe('pending')
  expect(decideConceptCandidate).toHaveBeenLastCalledWith('fixture-photo', 'bridge', 'clearRejection', 'detailId')
  app.unmount()
})

it('allows explicit negatives after empty suggestions but never retries uncertain failures', async () => {
  const { app, state } = mount()
  vi.mocked(requestConceptCandidates).mockResolvedValueOnce([])
  await state.generate()
  state.custom = 'bridge'
  vi.mocked(decideConceptCandidate).mockRejectedValueOnce(new Error('conflict or disconnected'))
  await state.addCustom('reject')
  expect(decideConceptCandidate).toHaveBeenCalledExactlyOnceWith('fixture-photo', 'bridge', 'reject', 'detailId')
  expect(state.candidates).toEqual([])
  expect(state.custom).toBe('bridge')
  expect(state.error).toContain('may have been saved')
  await state.addCustom('reject')
  expect(decideConceptCandidate).toHaveBeenCalledTimes(1)
  app.unmount()
})

it('does not save an owner-entered negative when read-only or anonymous', async () => {
  const { app, state } = mount()
  state.custom = 'bridge'
  for (const config of [{ isAdmin: true, readOnlyMode: true }, { isAdmin: false, readOnlyMode: false }]) {
    useConfigStore('mainId').config = config as AppConfig
    await nextTick()
    await state.addCustom('reject')
    expect(decideConceptCandidate).not.toHaveBeenCalled()
    expect(state.candidates).toEqual([])
  }
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

it('explains that a failed request may be busy or timed out without saving feedback', async () => {
  const { app, state } = mount()
  vi.mocked(requestConceptCandidates).mockRejectedValueOnce(new Error('offline or timeout'))
  await state.generate()
  expect(state.error).toContain('busy, or the request took too long')
  expect(state.error).toContain('try again')
  expect(state.candidates).toEqual([])
  expect(decideConceptCandidate).not.toHaveBeenCalled()
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
  expect(html).toContain('Not in this photo')
  expect(html).toContain('No suggestion is required')
})
