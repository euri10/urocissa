import axios from 'axios'
import { useConfigStore } from '@/store/configStore'
import { useAlbumStore } from '@/store/albumStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { useRouteResourceStore } from '@/store/routeResourceStore'
import { updateCachedResource } from '@/script/utils/routeResourceCache'
import type { GalleryAlbum } from '@/type/types'

export type AlbumAudienceAction = 'publish' | 'publishAll' | 'private'

export async function setAlbumAudience(
  albumId: string,
  action: AlbumAudienceAction
): Promise<void> {
  if (!useConfigStore('mainId').canWrite) throw new Error('Album publishing requires owner access.')
  const audience: GalleryAlbum['audience'] = action === 'private' ? 'private' : 'public'

  await axios.put('/put/album_audience', {
    albumId,
    audience,
    publishPhotos: action === 'publishAll'
  }, { timeout: 45_000 })

  updateCachedResource(albumId, (data) => {
    if (data.type === 'album') data.audience = audience
  })
  for (const isolationId of ['mainId', 'subId', 'tempId', 'shareId'] as const) {
    const album = useAlbumStore(isolationId).albums.get(albumId)
    if (album !== undefined) album.audience = audience
  }

  // Audience changes invalidate timestamp tokens for every collection and detail.
  const reloadStore = useCollectionReloadStore('mainId')
  reloadStore.requestMainCollectionReload()
  reloadStore.requestSubCollectionReload()
  await Promise.all((['detailId', 'subDetailId'] as const).map(async (isolationId) => {
    const store = useRouteResourceStore(isolationId)
    if (store.requestedId !== null) await store.load(store.requestedId, true)
  }))
}
