import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import axios from 'axios'
import { createPinia, setActivePinia } from 'pinia'
import { editAutoTags, fetchAutoTags } from './autoTags'
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
    const tag = { concept: 'cat', label: 'chat', confidence: 0.91, displayed: true }
    const get = vi.spyOn(axios, 'get').mockResolvedValue({
      data: { autoTagsEnabled: true, autoTagsPending: false, autoTags: [tag] }
    })
    expect(await fetchAutoTags('photo', new AbortController().signal)).toBe(false)
    expect(useDataStore('detailId').data.get(0)).toMatchObject({ autoTags: [tag] })
    expect(useDataStore('shareId').data.get(0)).not.toHaveProperty('autoTags')
    get.mockResolvedValue({ data: { autoTagsEnabled: true, autoTagsPending: false, autoTags: [{ ...tag, confidence: 2 }] } })
    await expect(fetchAutoTags('photo', new AbortController().signal)).rejects.toThrow()
  })
})
