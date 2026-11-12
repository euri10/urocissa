import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import axios from 'axios'
import { createPinia, setActivePinia } from 'pinia'
import { decideAutoTag, editAutoTags, fetchAutoTags, previewAutoTagSelection, getAutoTagStatus, recalculateAutoTags } from './autoTags'
import { useConfigStore } from '@/store/configStore'
import { useDataStore } from '@/store/dataStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { BackendDataParser } from '@/type/schemas'
import type { AppConfig } from './config'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))

describe('owner Auto tags', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: false } as AppConfig
    const photo = {
      ...BackendDataParser.parse({ type: 'image', id: 'photo', pending: false,
        width: 10, height: 10, size: 10, ext: 'jpg', tags: ['my-tag'] }),
      thumbhashUrl: null, timestamp: 1
    }
    for (const id of ['mainId', 'detailId', 'shareId'] as const) {
      useDataStore(id).data.set(0, { ...photo })
      useDataStore(id).hashMapData.set('photo', 0)
    }
  })
  afterEach(() => { vi.restoreAllMocks() })

  it('validates completion counts and uses the guarded recalculation endpoints', async () => {
    const status = { running: false, upToDate: true, lastCompletedAt: 1000, changes: { added: 2, removed: 1 }, lastError: null }
    const get = vi.spyOn(axios, 'get').mockResolvedValue({ data: status })
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ data: status })
    const signal = new AbortController().signal
    expect(await getAutoTagStatus(signal)).toEqual(status)
    expect(await recalculateAutoTags(signal)).toEqual(status)
    expect(get).toHaveBeenCalledWith('/get/auto-tag-status', { signal, timeout: 10000 })
    expect(put).toHaveBeenCalledWith('/put/recalculate_auto_tags', null, { signal, timeout: 60000 })
    get.mockResolvedValue({ data: { ...status, changes: { added: -1, removed: 0 } } })
    await expect(getAutoTagStatus()).rejects.toThrow()
  })

  it('turns cached photos off in place without editing User tags or share records', async () => {
    const put = vi.spyOn(axios, 'put').mockResolvedValue({})
    await editAutoTags('photo', false, 'detailId')
    expect(put).toHaveBeenCalledWith('/put/edit_auto_tags', { hash: 'photo', enabled: false })
    for (const id of ['mainId', 'detailId'] as const) {
      expect(useDataStore(id).data.get(0)).toMatchObject({
        autoTagsEnabled: false, autoTagsPending: false, autoTags: [], tags: ['my-tag']
      })
    }
    expect(useDataStore('shareId').data.get(0)).not.toHaveProperty('autoTags')
  })

  it('requires owner write permission and a ready timestamp for selections', async () => {
    const put = vi.spyOn(axios, 'put').mockResolvedValue({})
    await editAutoTags([0], false, 'mainId')
    expect(put).not.toHaveBeenCalled()
    usePrefetchStore('mainId').timestamp = 7
    usePrefetchStore('mainId').dataLength = 1
    await editAutoTags([0], true, 'mainId')
    expect(put).toHaveBeenCalledWith('/put/edit_auto_tags', {
      timestamp: 7, selection: { mode: 'explicit', indices: [0] }, enabled: true
    })
    put.mockClear()
    useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: true } as AppConfig
    await editAutoTags('photo', false, 'detailId')
    expect(put).not.toHaveBeenCalled()
  })

  it('validates confidence and updates only owner caches when predictions arrive', async () => {
    const tag = { concept: 'cat', label: 'cat', confidence: 0.91, displayed: true }
    const get = vi.spyOn(axios, 'get').mockResolvedValue({
      data: { autoTagsEnabled: true, autoTagsPending: false, autoTags: [tag] }
    })
    expect(await fetchAutoTags('photo', new AbortController().signal)).toBe(false)
    expect(useDataStore('detailId').data.get(0)).toMatchObject({ autoTags: [tag] })
    expect(useDataStore('shareId').data.get(0)).not.toHaveProperty('autoTags')
    get.mockResolvedValue({ data: { autoTagsEnabled: true, autoTagsPending: false, autoTags: [{ ...tag, confidence: 2 }] } })
    await expect(fetchAutoTags('photo', new AbortController().signal)).rejects.toThrow()
  })
  it('confirms the canonical English User tag across owner caches and refreshes collections/facets', async () => {
    for (const id of ['mainId', 'detailId'] as const) {
      const photo = useDataStore(id).data.get(0)
      if (photo?.type === 'image') photo.autoTags = [{ concept: 'cat', label: 'cat', confidence: 0.91, displayed: true }]
    }
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ data: { photos: 1, label: 'cat' } })
    vi.spyOn(axios, 'get').mockResolvedValue({ status: 200, data: { tags: [{ value: 'cat', count: 1 }], makes: [], models: [] } })
    expect(await decideAutoTag('photo', 'cat', 'confirm', 'detailId')).toBe(true)
    expect(put).toHaveBeenCalledWith('/put/auto_tag_decision', { hash: 'photo', concept: 'cat', action: 'confirm' })
    expect(useDataStore('detailId').data.get(0)?.tags).toContain('cat')
    for (const id of ['mainId', 'detailId'] as const) {
      expect(useDataStore(id).data.get(0)).toMatchObject({ tags: ['my-tag', 'cat'], autoTags: [] })
    }
    // Share caches contain only server-projected metadata; no Auto state is written into them.
    expect(useDataStore('shareId').data.get(0)).not.toHaveProperty('autoTags')
  })
  it('sends compact bulk decisions and rejects non-owner/share writes', async () => {
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ data: { photos: 500, label: 'cat' } })
    vi.spyOn(axios, 'get').mockResolvedValue({ status: 200, data: { tags: [], makes: [], models: [] } })
    usePrefetchStore('mainId').timestamp = 7
    usePrefetchStore('mainId').dataLength = 500
    for (const id of ['mainId', 'detailId'] as const) {
      const photo = useDataStore(id).data.get(0)
      if (photo?.type === 'image') photo.autoTags = [{ concept: 'cat', label: 'cat', confidence: 0.91, displayed: true }]
    }
    await decideAutoTag({ mode: 'allExcept', excludedIndices: [] }, 'cat', 'reject', 'mainId')
    expect(put).toHaveBeenCalledWith('/put/auto_tag_decision', { timestamp: 7, selection: { mode: 'allExcept', excludedIndices: [] }, concept: 'cat', action: 'reject' })
    expect(useDataStore('mainId').data.get(0)?.tags).not.toContain('cat')
    expect(useDataStore('detailId').data.get(0)).toMatchObject({ autoTags: [], tags: ['my-tag'] })
    put.mockClear()
    expect(await decideAutoTag('photo', 'cat', 'confirm', 'shareId')).toBe(false)
    useConfigStore('mainId').config = { isAdmin: false, readOnlyMode: false } as AppConfig
    expect(await decideAutoTag('photo', 'cat', 'confirm', 'mainId')).toBe(false)
    expect(put).not.toHaveBeenCalled()
  })
  it('previews exact photos and published-photo counts and preserves caches on failure', async () => {
    vi.spyOn(axios, 'post').mockResolvedValue({ data: { photos: 500, publicPhotos: 31, concepts: [{ concept: 'cat', label: 'cat' }] } })
    expect((await previewAutoTagSelection('photo', 'detailId')).publicPhotos).toBe(31)
    vi.spyOn(axios, 'put').mockRejectedValue(new Error('offline'))
    await expect(decideAutoTag('photo', 'cat', 'confirm', 'detailId')).rejects.toThrow('offline')
    expect(useDataStore('mainId').data.get(0)?.tags).not.toContain('cat')
  })
})
