import { beforeEach, describe, expect, it, vi } from 'vitest'
import axios from 'axios'
import { createPinia, setActivePinia } from 'pinia'
import { setAlbumAudience } from './albumAudience'
import { useConfigStore } from '@/store/configStore'
import { useDataStore } from '@/store/dataStore'
import { useAlbumStore } from '@/store/albumStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { albumInfoSchema, BackendDataParser } from '@/type/schemas'
import type { AppConfig } from './config'

const routes = vi.hoisted(() => ({
  detailId: { requestedId: 'album-a', load: vi.fn() },
  subDetailId: { requestedId: 'photo-a', load: vi.fn() }
}))
vi.mock('@/store/routeResourceStore', () => ({
  useRouteResourceStore: (id: keyof typeof routes) => routes[id]
}))

function audience(isolationId: 'mainId' | 'detailId') {
  const data = useDataStore(isolationId).data.get(0)
  if (data?.type !== 'album') throw new Error('expected album')
  return data.audience
}

describe('album audience mutations', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.restoreAllMocks()
    vi.clearAllMocks()
    useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: false } as AppConfig
    for (const isolationId of ['mainId', 'detailId'] as const) {
      const store = useDataStore(isolationId)
      store.data.set(0, {
        ...BackendDataParser.parse({
          type: 'album', id: 'album-a', pending: false, title: 'Fixture album',
          startTime: null, endTime: null, lastModifiedTime: 1, cover: null,
          itemCount: 2, itemSize: 100
        }),
        thumbhashUrl: null,
        timestamp: 1
      })
      store.hashMapData.set('album-a', 0)
    }
    useAlbumStore('mainId').albums.set('album-a', albumInfoSchema.parse({
      albumId: 'album-a', albumName: 'Fixture album', shareList: {}
    }))
  })

  it.each([
    ['publish', 'public', false],
    ['publishAll', 'public', true],
    ['private', 'private', false]
  ] as const)('submits %s with explicit photo publication=%s', async (action, expectedAudience, publishPhotos) => {
    const put = vi.spyOn(axios, 'put').mockResolvedValue({ status: 200 })
    await setAlbumAudience('album-a', action)
    expect(put).toHaveBeenCalledWith('/put/album_audience', {
      albumId: 'album-a', audience: expectedAudience, publishPhotos
    }, { timeout: 45_000 })
    expect(audience('mainId')).toBe(expectedAudience)
    expect(audience('detailId')).toBe(expectedAudience)
    expect(useAlbumStore('mainId').albums.get('album-a')?.audience).toBe(expectedAudience)
    expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(1)
    expect(useCollectionReloadStore('mainId').subCollectionReload).toBe(1)
    expect(routes.detailId.load).toHaveBeenCalledWith('album-a', true)
    expect(routes.subDetailId.load).toHaveBeenCalledWith('photo-a', true)
  })

  it('keeps cached audience and snapshots unchanged when rejected', async () => {
    vi.spyOn(axios, 'put').mockRejectedValueOnce(new Error('Fixture rejection'))
    await expect(setAlbumAudience('album-a', 'publishAll')).rejects.toThrow('Fixture rejection')
    expect(audience('mainId')).toBe('private')
    expect(audience('detailId')).toBe('private')
    expect(useCollectionReloadStore('mainId').mainCollectionReload).toBe(0)
    expect(routes.detailId.load).not.toHaveBeenCalled()
  })

  it.each([
    undefined,
    { isAdmin: false, readOnlyMode: false },
    { isAdmin: true, readOnlyMode: true }
  ])('blocks publication without writable owner config %j', async (config) => {
    useConfigStore('mainId').config = config as AppConfig | undefined
    const put = vi.spyOn(axios, 'put')
    await expect(setAlbumAudience('album-a', 'publishAll')).rejects.toThrow('owner access')
    expect(put).not.toHaveBeenCalled()
  })
})
