import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createRenderer, createSSRApp, h, ssrContextKey, type Component, type ComponentPublicInstance, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import ConceptReviewPage from './ConceptReviewPage.vue'
import ReviewToolbar from './ConceptReview/ReviewToolbar.vue'
import { useConfigStore } from '@/store/configStore'
import { useCollectionStore } from '@/store/collectionStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useInitializedStore } from '@/store/initializedStore'
import { decideAutoTag, previewAutoTagSelection } from '@/api/autoTags'
import type { AppConfig } from '@/api/config'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))
vi.mock('vue-router', () => ({ useRoute: () => ({ query: { concept: 'cat' } }) }))
vi.mock('@/api/autoTags', () => ({ decideAutoTag: vi.fn(), previewAutoTagSelection: vi.fn() }))
vi.mock('@/components/Home/Home.vue', () => ({ default: { render: () => h('div', { 'data-gallery': true }) } }))
vi.mock('./PageLayout/PageTemplate.vue', () => ({ default: (_: unknown, { slots }: { slots: { content: () => unknown } }) => slots.content() }))

const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText: () => undefined, setElementText: () => undefined, patchProp: () => undefined,
  parentNode: () => null, nextSibling: () => null, insert: () => undefined, remove: () => undefined
})
interface State {
  open: (action: 'confirm' | 'reject') => Promise<void>
  save: () => Promise<void>
  opened: boolean
  busy: boolean
  error: string | null
}
const summary = { concept: 'cat', label: 'chat', positiveExamples: 4, negativeExamples: 3, learningMode: 'builtin' as const }
const stub: FunctionalComponent = (_props, { attrs, slots }) => h('div', attrs, [slots.title?.(), slots.subtitle?.(), slots.default?.()])

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: false } as AppConfig
  const prefetch = usePrefetchStore('mainId')
  prefetch.timestamp = 7
  prefetch.dataLength = 3
  prefetch.conceptReview = summary
  useInitializedStore('mainId').initialized = true
  useCollectionStore('mainId').enterEdit(0)
  useCollectionStore('mainId').addApi(2)
  vi.mocked(previewAutoTagSelection).mockResolvedValue({ photos: 2, publicPhotos: 1, concepts: [{ concept: 'cat', label: 'chat' }] })
  vi.mocked(decideAutoTag).mockResolvedValue(true)
})

function mount() {
  let state: State | undefined
  const Subject: Component = { ...ConceptReviewPage, render: () => null, created(this: ComponentPublicInstance) {
    state = (this.$ as unknown as { setupState: State }).setupState
  } }
  const app = renderer.createApp(Subject)
  app.provide(ssrContextKey, { modules: new Set<string>() })
  app.mount({})
  if (state === undefined) throw new Error('Review did not mount')
  return { app, state }
}

describe('Concept review', () => {
  it.each(['confirm', 'reject'] as const)('previews then bulk-%s the exact selected snapshot', async (action) => {
    const { app, state } = mount()
    await state.open(action)
    expect(previewAutoTagSelection).toHaveBeenCalledWith({ mode: 'explicit', indices: [0, 2] }, 'mainId')
    useCollectionStore('mainId').addApi(1)
    await state.save()
    expect(decideAutoTag).toHaveBeenCalledWith({ mode: 'explicit', indices: [0, 2] }, 'cat', action, 'mainId')
    expect(state.opened).toBe(false)
    app.unmount()
  })
  it('refuses to reinterpret an old preview against a refreshed grid', async () => {
    const { app, state } = mount()
    await state.open('confirm')
    usePrefetchStore('mainId').timestamp = 8
    await state.save()
    expect(decideAutoTag).not.toHaveBeenCalled()
    expect(state.opened).toBe(true)
    expect(state.error).toContain('review changed')
    app.unmount()
  })
  it('keeps failed actions open and selections intact, and refuses read-only writes', async () => {
    const { app, state } = mount()
    await state.open('reject')
    vi.mocked(decideAutoTag).mockRejectedValue(new Error('offline'))
    await state.save()
    expect(state.opened).toBe(true)
    expect(state.error).toContain('Could not save')
    expect(useCollectionStore('mainId').descriptor()).toEqual({ mode: 'explicit', indices: [0, 2] })
    vi.mocked(decideAutoTag).mockClear()
    useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: true } as AppConfig
    await state.save()
    expect(decideAutoTag).not.toHaveBeenCalled()
    app.unmount()
  })
  it('renders learning counts, fixed confidence order, and accessible bulk actions', async () => {
    const app = createSSRApp(ReviewToolbar, { summary, total: 3, selected: 2, selecting: true, ready: true, failed: false, canWrite: true })
    for (const name of ['VSheet', 'VCardItem', 'VCardTitle', 'VCardSubtitle', 'VChip', 'VBtn']) app.component(name, stub)
    const html = await renderToString(app)
    expect(html).toContain('Least confident first')
    expect(html).toContain('4/5 positive examples')
    expect(html).toContain('3/5 negative examples')
    expect(html).toContain('Confirm selected')
    expect(html).toContain('Reject selected')
  })
  it.each([
    ['inactive', 4, 0, 'Tag 1 more photo to start suggesting similar matches.'],
    ['similarity', 5, 0, 'Matching similar photos using your tagged examples.'],
    ['learned', 5, 5, 'Learning from your confirmations and rejections.']
  ] as const)('explains the actual %s learning state', async (learningMode, positiveExamples, negativeExamples, text) => {
    const app = createSSRApp(ReviewToolbar, { summary: { ...summary, concept: 'personal:fixture', learningMode, positiveExamples, negativeExamples }, total: 0, selected: 0, selecting: false, ready: true, failed: false, canWrite: true })
    for (const name of ['VSheet', 'VCardItem', 'VCardTitle', 'VCardSubtitle', 'VChip', 'VBtn']) app.component(name, stub)
    expect(await renderToString(app)).toContain(text)
  })
  it('never renders the gallery for a visitor', async () => {
    useConfigStore('mainId').config = { isAdmin: false, readOnlyMode: false } as AppConfig
    const app = createSSRApp(ConceptReviewPage)
    for (const name of ['VDialog', 'VCard', 'VCardItem', 'VCardTitle', 'VCardText', 'VCardActions', 'VBtn', 'VSpacer']) app.component(name, stub)
    expect(await renderToString(app)).not.toContain('data-gallery')
  })
})
