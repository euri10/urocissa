import { beforeEach, describe, expect, it, vi } from 'vitest'
import axios from 'axios'
import { createPinia, setActivePinia } from 'pinia'
import { editPhotoAudience } from './editPhotoAudience'
import { useConfigStore } from '@/store/configStore'
import { useDataStore } from '@/store/dataStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useTokenStore } from '@/store/tokenStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { useScrollTopStore } from '@/store/scrollTopStore'
import { useLocationStore } from '@/store/locationStore'
import { useFilterStore } from '@/store/filterStore'
import { BackendDataParser } from '@/type/schemas'
import type { AppConfig } from '@/api/config'

const direct = vi.hoisted(() => ({ requestedId: 'a'.repeat(64), load: vi.fn() }))
vi.mock('@/store/routeResourceStore', () => ({ useRouteResourceStore: () => direct }))
vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))

function cachedAudience(id: 'mainId' | 'detailId') {
  const data = useDataStore(id).data.get(0)
  return data?.type === 'album' ? undefined : data?.audience
}

describe('photo audience mutations', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.restoreAllMocks()
    direct.load.mockClear()
    useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: false } as AppConfig
    const photo = {
      ...BackendDataParser.parse({
        type: 'image', id: direct.requestedId, pending: false, width: 10, height: 10,
        ext: 'jpg', size: 10, audience: 'private'
      }),
      thumbhashUrl: null, timestamp: 1
    }
    for (const id of ['mainId', 'detailId'] as const) {
      useDataStore(id).data.set(0, { ...photo })
      useDataStore(id).hashMapData.set(photo.id, 0)
      useTokenStore(id).timestampToken = 'old-snapshot'
    }
    usePrefetchStore('mainId').timestamp = 10
    usePrefetchStore('mainId').dataLength = 10
    useCollectionStore('mainId').addApi(0)
  })

  it.each(['public', 'private'] as const)('sets an all-except selection to %s without resetting the owner view', async (audience) => {
    const data = useDataStore('mainId').data
    const photo = data.get(0)
    if (photo === undefined) throw new Error('Expected a cached photo')
    usePrefetchStore('mainId').dataLength = 2
    data.set(1, { ...photo, id: 'b'.repeat(64), audience: 'private' })
    useScrollTopStore('mainId').scrollTop = 4200
    useLocationStore('mainId').locationIndex = 42
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    await editPhotoAudience({ mode: 'allExcept', excludedIndices: [1] }, audience, 'mainId')
    expect(put).toHaveBeenCalledWith('/put/edit_photo_audience', {
      selection: { mode: 'allExcept', excludedIndices: [1] }, timestamp: 10, audience
    })
    expect(cachedAudience('mainId')).toBe(audience)
    expect(cachedAudience('detailId')).toBe(audience)
    expect(data.get(1)).toMatchObject({ audience: 'private' })
    expect(data.get(0)).toBe(photo)
    expect(usePrefetchStore('mainId').timestamp).toBe(10)
    expect(useTokenStore('mainId').timestampToken).toBe('old-snapshot')
    expect(useTokenStore('detailId').timestampToken).toBe('old-snapshot')
    expect(useCollectionStore('mainId').selectionSet.has(0)).toBe(true)
    expect(useScrollTopStore('mainId').scrollTop).toBe(4200)
    expect(useLocationStore('mainId').locationIndex).toBe(42)
    const reload = useCollectionReloadStore('mainId')
    expect(reload.mainCollectionReload).toBe(0)
    expect(reload.subCollectionReload).toBe(0)
    expect(direct.load).not.toHaveBeenCalled()
  })

  it('updates a direct photo in place without a collection timestamp', async () => {
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    const photo = useDataStore('detailId').data.get(0)
    await editPhotoAudience(direct.requestedId, 'private', 'detailId')
    expect(put).toHaveBeenCalledWith('/put/edit_photo_audience', {
      hash: direct.requestedId, audience: 'private'
    })
    expect(useDataStore('detailId').data.get(0)).toBe(photo)
    expect(cachedAudience('detailId')).toBe('private')
    expect(useTokenStore('detailId').timestampToken).toBe('old-snapshot')
    expect(direct.load).not.toHaveBeenCalled()
  })

  it('can publish then make the retained selection private using the same snapshot', async () => {
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    const collection = useCollectionStore('mainId')
    for (const audience of ['public', 'private'] as const) {
      await editPhotoAudience([...collection.selectionSet], audience, 'mainId')
      expect(put).toHaveBeenLastCalledWith('/put/edit_photo_audience', {
        selection: { mode: 'explicit', indices: [0] }, timestamp: 10, audience
      })
      expect(cachedAudience('mainId')).toBe(audience)
    }
    expect(put).toHaveBeenCalledTimes(2)
  })

  it.each([false, true])('updates newly cached selected photos only in the original snapshot; navigated=%s', async (navigated) => {
    let accept: () => void = () => { throw new Error('PUT has not started') }
    vi.spyOn(axios, 'put').mockImplementation(() => new Promise((resolve) => {
      accept = () => { resolve({ status: 200 }) }
    }))
    usePrefetchStore('mainId').dataLength = 2
    const pending = editPhotoAudience({ mode: 'allExcept', excludedIndices: [] }, 'public', 'mainId')
    const store = useDataStore('mainId')
    const photo = store.data.get(0)
    if (photo === undefined) throw new Error('Expected a cached photo')
    if (navigated) usePrefetchStore('mainId').timestamp = 20
    store.data.set(1, { ...photo, id: 'c'.repeat(64) })
    store.hashMapData.set('c'.repeat(64), 1)
    accept()
    await pending
    expect(store.data.get(1)).toMatchObject({ audience: navigated ? 'private' : 'public' })
    expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(navigated ? 1 : 0)
  })

  it('retains the refresh fallback for bulk selections containing unloaded photos', async () => {
    vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    await editPhotoAudience({ mode: 'allExcept', excludedIndices: [] }, 'private', 'mainId')
    expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(1)
    expect(useCollectionReloadStore('mainId').subCollectionReload).toBe(1)
    expect(direct.load).toHaveBeenCalledWith(direct.requestedId, true)
  })

  it.each(['mainId', 'subId'] as const)('refreshes only the audience-filtered collection: %s', async (isolationId) => {
    useFilterStore(isolationId).searchString = 'and(tag:"family", not(audience:"public"))'
    vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    await editPhotoAudience(direct.requestedId, 'public', 'detailId')
    const reload = useCollectionReloadStore('mainId')
    expect(reload.mainCollectionReload).toBe(isolationId === 'mainId' ? 1 : 0)
    expect(reload.subCollectionReload).toBe(isolationId === 'subId' ? 1 : 0)
    expect(direct.load).not.toHaveBeenCalled()
  })

  it('does not reload for audience-like text in an ordinary keyword search', async () => {
    useFilterStore('mainId').searchString = 'any:"audience:private"'
    vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    await editPhotoAudience([0], 'public', 'mainId')
    expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(0)
  })

  it('keeps audience, selection and snapshots unchanged on failure', async () => {
    vi.spyOn(axios, 'put').mockRejectedValue(new Error('fixture failure'))
    await editPhotoAudience([0], 'public', 'mainId')
    expect(cachedAudience('mainId')).toBe('private')
    expect(cachedAudience('detailId')).toBe('private')
    expect(useTokenStore('mainId').timestampToken).toBe('old-snapshot')
    expect(useCollectionStore('mainId').selectionSet.has(0)).toBe(true)
    expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(0)
    expect(direct.load).not.toHaveBeenCalled()
  })

  it.each([undefined, { isAdmin: false }, { isAdmin: true, readOnlyMode: true }])(
    'refuses writes when owner permission is absent or read-only: %s', async (config) => {
      useConfigStore('mainId').config = config as AppConfig | undefined
      const put = vi.spyOn(axios, 'put')
      await editPhotoAudience(direct.requestedId, 'public', 'detailId')
      expect(put).not.toHaveBeenCalled()
    }
  )

  it('does not submit an empty or unavailable batch', async () => {
    const put = vi.spyOn(axios, 'put')
    await editPhotoAudience([], 'public', 'mainId')
    usePrefetchStore('mainId').timestamp = null
    await editPhotoAudience([0], 'public', 'mainId')
    expect(put).not.toHaveBeenCalled()
  })
})
