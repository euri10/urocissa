/* eslint-disable vue/one-component-per-file, @typescript-eslint/unbound-method -- Test rendering adapters and mocked HTTP methods. */
import { expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { compile } from '@vue/compiler-dom'
import { compileScript, parse } from '@vue/compiler-sfc'
import * as Vue from 'vue'
import { createRenderer, h, nextTick, reactive, ssrContextKey, watch, type ComponentPublicInstance, type FunctionalComponent, type PropType, type VNode } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import axios from 'axios'
import ViewPage from './ViewPage.vue'
import ConceptBuilder from './Metadata/ConceptBuilder.vue'
import { useConfigStore } from '@/store/configStore'
import { useConstStore } from '@/store/constStore'
import { useDataStore } from '@/store/dataStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { BackendDataParser } from '@/type/schemas'
import type { AppConfig } from '@/api/config'
import type { GalleryImage, IsolationId } from '@/type/types'
import type { Candidate, ConceptReviewState } from './Metadata/conceptReviewState'

// Exercise the production ViewPage key/provider and real decision/reload API.
// Media decoding and Vuetify rendering are outside this state-lifecycle test.
vi.mock('vue-router', () => ({ useRoute: () => route, useRouter: () => ({ back: vi.fn() }), onBeforeRouteLeave: vi.fn() }))
vi.mock('axios', () => ({ default: { post: vi.fn(), put: vi.fn(), get: vi.fn(), isAxiosError: vi.fn() } }))
vi.mock('@/api/fetchRouteResource', () => ({ fetchRouteResource: vi.fn() }))
vi.mock('@/store/workerStore', () => ({ useWorkerStore: () => ({ worker: null, imgWorker: [], initializeWorker: vi.fn(), terminateWorker: vi.fn() }) }))
vi.mock('./Display/Display.vue', () => ({ default: { render: () => null } }))
vi.mock('./Metadata/ViewPageMetadata.vue', async () => {
  const { defineComponent, h } = await import('vue')
  const { default: Builder } = await import('./Metadata/ConceptBuilder.vue')
  Builder.render = () => null
  return { default: defineComponent({
    props: { abstractData: { type: Object as PropType<GalleryImage>, required: true }, isolationId: { type: String as PropType<IsolationId>, required: true } },
    setup: props => () => h(Builder, { photo: props.abstractData, isolationId: props.isolationId })
  }) }
})

const route = reactive({ params: { hash: 'fixture-photo' } })
// Node-mode Vite imports SSR templates. Compile the unchanged real client
// template so its keyed subtree actually mounts/unmounts in this regression.
const { descriptor } = parse(readFileSync(new URL('./ViewPage.vue', import.meta.url), 'utf8'))
if (descriptor.template === null) throw new Error('Missing real viewer template')
const script = compileScript(descriptor, { id: 'viewer-concept-lifecycle' })
const client = compile(descriptor.template.content, {
  mode: 'function', prefixIdentifiers: true, bindingMetadata: script.bindings
})
// eslint-disable-next-line @typescript-eslint/no-implied-eval -- Only the repository's real template, compiled above; no external input.
const compileRender = new Function('Vue', client.code) as (runtime: typeof Vue) => (...args: unknown[]) => VNode
const Viewer = { ...ViewPage, render: compileRender(Vue) }
const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText: () => undefined, setElementText: () => undefined, patchProp: () => undefined,
  parentNode: () => null, nextSibling: () => null, insert: () => undefined, remove: () => undefined
})
const surface: FunctionalComponent = (_props, { slots }) => h('div', slots.default?.())
interface State extends Pick<ConceptReviewState, 'candidates' | 'loaded' | 'custom'> {
  generate: () => Promise<void>
  decide: (candidate: Candidate, action: 'confirm' | 'reject') => Promise<void>
  addCustom: () => Promise<void>
}

it.each([false, true])('keeps every decision through actual viewer collection handoffs (mobile=%s)', async isMobile => {
  vi.clearAllMocks()
  setActivePinia(createPinia())
  vi.stubGlobal('navigator', { hardwareConcurrency: 8 })
  route.params.hash = 'fixture-photo'
  const config = useConfigStore('mainId')
  config.config = { isAdmin: true, readOnlyMode: false } as AppConfig
  config.isMobile = isMobile
  useConstStore('mainId').showInfo = true
  const main = useDataStore('mainId')
  const detail = useDataStore('detailId')
  const fixture = { ...BackendDataParser.parse({ id: route.params.hash, type: 'image', pending: false,
    autoTagsEnabled: true, width: 10, height: 10, size: 10, ext: 'jpg', tags: [] }), thumbhashUrl: null, timestamp: 1 }
  function refill() {
    main.data.set(7, { ...fixture, tags: [...fixture.tags] })
    main.hashMapData.set(fixture.id, 7)
  }
  refill()
  detail.data.set(0, { ...fixture, tags: [] })
  detail.hashMapData.set(fixture.id, 0)
  vi.mocked(axios.post).mockResolvedValue({ data: { candidates: ['tree', 'water'], decisions: {} } })
  vi.mocked(axios.put).mockImplementation((_url, data) => {
    const payload = data as { label: string }
    return Promise.resolve({ data: { photos: 1, label: payload.label, concept: payload.label } })
  })
  vi.mocked(axios.get).mockResolvedValue({ status: 200, data: { tags: [], makes: [], models: [] } })
  const reload = useCollectionReloadStore('mainId')
  const stop = watch(() => reload.mainCollectionReload, () => { main.clearAll() }, { flush: 'sync' })
  let current: State | undefined
  let mounts = 0
  const app = renderer.createApp(Viewer, { collectionIsolationId: 'mainId', directIsolationId: 'detailId', hashParam: 'hash' })
  app.mixin({ created(this: ComponentPublicInstance) {
    if (this.$.type === ConceptBuilder) {
      current = (this.$ as unknown as { setupState: State }).setupState
      mounts += 1
    }
  } })
  for (const name of ['VOverlay', 'VSheet', 'VListItem', 'VAlert', 'VBtn', 'VCardSubtitle', 'VChip', 'VTextField', 'VProgressLinear']) app.component(name, surface)
  app.component('RouterView', { render: () => null })
  app.provide(ssrContextKey, { modules: new Set<string>() })
  app.mount({})
  function state(): State {
    if (current === undefined) throw new Error('Real viewer did not mount ConceptBuilder')
    return current
  }
  try {
    await state().generate()
    for (const [label, action] of [['tree', 'confirm'], ['water', 'reject']] as const) {
      refill()
      await nextTick()
      const candidate = state().candidates.find(item => item.label === label)
      if (candidate === undefined) throw new Error('Viewer lost its remaining suggestions')
      await state().decide(candidate, action)
      await nextTick()
      expect(state().candidates.find(item => item.label === label)?.decision).toBe(action === 'confirm' ? 'confirmed' : 'rejected')
    }
    refill()
    await nextTick()
    state().custom = 'rhubarb'
    await state().addCustom()
    await nextTick()
    expect(state().candidates).toEqual([
      { label: 'tree', decision: 'confirmed' }, { label: 'water', decision: 'rejected' },
      { label: 'rhubarb', decision: 'confirmed' }
    ])
    expect(mounts).toBeGreaterThanOrEqual(6)
    expect(axios.post).toHaveBeenCalledTimes(1)
    expect(axios.put).toHaveBeenCalledTimes(3)
    route.params.hash = 'other-photo'
    detail.data.set(1, { ...fixture, id: 'other-photo' })
    detail.hashMapData.set('other-photo', 1)
    await nextTick()
    expect(state().candidates).toEqual([])
    expect(state().loaded).toBe(false)
    expect(axios.post).toHaveBeenCalledTimes(1)
  } finally {
    stop()
    app.unmount()
    vi.unstubAllGlobals()
  }
})
