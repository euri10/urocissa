import axios from 'axios'
import { beforeEach, expect, it, vi } from 'vitest'
import { createRenderer, nextTick, ssrContextKey, type Component, type ComponentPublicInstance } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { decideConceptCandidate } from '@/api/autoTags'
import { useConfigStore } from '@/store/configStore'
import type { AppConfig } from '@/api/config'
import ItemSuggestedTags from './ItemSuggestedTags.vue'

vi.mock('@/api/autoTags', () => ({ decideConceptCandidate: vi.fn() }))
interface State {
  labels: string[]
  available: boolean
  decide: (label: string, action: 'confirm' | 'reject') => Promise<void>
}
const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
  createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
  setText: () => undefined, setElementText: () => undefined, patchProp: () => undefined,
  parentNode: () => null, nextSibling: () => null, insert: () => undefined, remove: () => undefined
})

beforeEach(() => {
  setActivePinia(createPinia())
  useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: false } as AppConfig
  vi.restoreAllMocks()
  vi.mocked(decideConceptCandidate).mockReset().mockResolvedValue(true)
})

it('loads the separate tier and records one-click owner decisions', async () => {
  const get = vi.spyOn(axios, 'get').mockResolvedValueOnce({ data: { suggestedTags: ['dog', 'tree'], pending: false, enabled: true } })
    .mockResolvedValue({ data: { suggestedTags: ['tree'], pending: false, enabled: true } })
  let state: State | undefined
  const subject: Component = { ...ItemSuggestedTags, render: () => null, created(this: ComponentPublicInstance) {
    state = (this.$ as unknown as { setupState: State }).setupState
  } }
  const app = renderer.createApp(subject, { photo: { id: 'fixture', type: 'image', tags: [] }, isolationId: 'detailId' })
  app.provide(ssrContextKey, { modules: new Set<string>() })
  app.mount({})
  await nextTick()
  await Promise.resolve()
  if (state === undefined) throw new Error('Suggested tags did not mount')
  expect(state.available).toBe(true)
  expect(state.labels).toEqual(['dog', 'tree'])
  await state.decide('dog', 'reject')
  expect(decideConceptCandidate).toHaveBeenCalledWith('fixture', 'dog', 'reject', 'detailId')
  expect(state.labels).toEqual(['tree'])
  await state.decide('tree', 'confirm')
  expect(decideConceptCandidate).toHaveBeenCalledWith('fixture', 'tree', 'confirm', 'detailId')
  expect(get).toHaveBeenCalledTimes(3)
  app.unmount()
})
