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

  it('publishes an all-except selection and refreshes every snapshot after acceptance', async () => {
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    await editPhotoAudience({ mode: 'allExcept', excludedIndices: [3] }, 'public', 'mainId')
    expect(put).toHaveBeenCalledWith('/put/edit_photo_audience', {
      selection: { mode: 'allExcept', excludedIndices: [3] }, timestamp: 10, audience: 'public'
    })
    expect(cachedAudience('mainId')).toBe('public')
    expect(cachedAudience('detailId')).toBe('public')
    expect(useTokenStore('mainId').timestampToken).toBeNull()
    expect(useTokenStore('detailId').timestampToken).toBeNull()
    expect(useCollectionStore('mainId').selectionSet.size).toBe(0)
    const reload = useCollectionReloadStore('mainId')
    expect(reload.mainCollectionReload).toBe(1)
    expect(reload.subCollectionReload).toBe(1)
    expect(direct.load).toHaveBeenCalledWith(direct.requestedId, true)
  })

  it('supports a direct photo without a collection timestamp', async () => {
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    await editPhotoAudience(direct.requestedId, 'private', 'detailId')
    expect(put).toHaveBeenCalledWith('/put/edit_photo_audience', {
      hash: direct.requestedId, audience: 'private'
    })
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
