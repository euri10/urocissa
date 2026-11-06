import axios from 'axios'
import { useConfigStore } from '@/store/configStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { useRouteResourceStore } from '@/store/routeResourceStore'
import { useFilterStore } from '@/store/filterStore'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import {
  selectedCachedResourceIds,
  updateCachedResource
} from '@/script/utils/routeResourceCache'
import { normalizeSelection, selectionCount, type SelectionInput } from '@/type/selection'
import type { IsolationId } from '@/type/types'

export type PhotoAudience = 'private' | 'public'

export async function editPhotoAudience(
  target: string | SelectionInput,
  audience: PhotoAudience,
  isolationId: IsolationId
): Promise<void> {
  if (!useConfigStore('mainId').canWrite) return
  const prefetch = usePrefetchStore(isolationId)
  const selection = typeof target === 'string' ? undefined : normalizeSelection(target)
  if (selection !== undefined && selectionCount(selection, prefetch.dataLength) === 0) return
  if (selection !== undefined && prefetch.timestamp === null) {
    useMessageStore('mainId').error('Cannot update audience because the collection is not ready.')
    return
  }
  const timestamp = prefetch.timestamp
  const count = selection === undefined ? 1 : selectionCount(selection, prefetch.dataLength)
  const ids = new Set(typeof target === 'string'
    ? [target]
    : selectedCachedResourceIds(isolationId, normalizeSelection(target)))

  await tryWithMessageStore('mainId', async () => {
    await axios.put('/put/edit_photo_audience', typeof target === 'string'
      ? { hash: target, audience }
      : { selection, timestamp, audience })

    // Include batches loaded during the request, but never reuse indices after navigation.
    if (selection !== undefined && prefetch.timestamp === timestamp) {
      for (const id of selectedCachedResourceIds(isolationId, selection)) ids.add(id)
    }
    for (const id of ids) {
      updateCachedResource(id, (data) => {
        if (data.type !== 'album') data.audience = audience
      })
    }
    // Unloaded batch members may also be cached in another collection. Without
    // their IDs, refresh those snapshots so they cannot keep stale audience badges.
    const reload = useCollectionReloadStore('mainId')
    const hasUnloadedSelection = ids.size < count
    if (hasUnloadedSelection || useFilterStore('mainId').hasAudienceFilter) {
      reload.requestMainCollectionReload()
    }
    if (hasUnloadedSelection || useFilterStore('subId').hasAudienceFilter) {
      reload.requestSubCollectionReload()
    }
    if (hasUnloadedSelection) {
      await Promise.all((['detailId', 'subDetailId'] as const).map(async (id) => {
        const resource = useRouteResourceStore(id)
        if (resource.requestedId !== null) await resource.load(resource.requestedId, true)
      }))
    }
    // Otherwise keep the owner grid, selection and detail view in place, as for tag edits.
    useMessageStore('mainId').success(audience === 'public' ? 'Photos published.' : 'Photos made private.')
  })
}
