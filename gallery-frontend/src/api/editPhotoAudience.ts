import axios from 'axios'
import { useConfigStore } from '@/store/configStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useTokenStore } from '@/store/tokenStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { useRouteResourceStore } from '@/store/routeResourceStore'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import {
  allResourceIsolationIds,
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
  const ids = typeof target === 'string'
    ? [target]
    : selectedCachedResourceIds(isolationId, normalizeSelection(target))

  await tryWithMessageStore('mainId', async () => {
    await axios.put('/put/edit_photo_audience', typeof target === 'string'
      ? { hash: target, audience }
      : { selection, timestamp: prefetch.timestamp, audience })

    for (const id of ids) {
      updateCachedResource(id, (data) => {
        if (data.type !== 'album') data.audience = audience
      })
    }
    // Every snapshot carries the audience revision, including unselected rows.
    // Refresh both collection hosts and direct views before reusing their tokens.
    for (const id of allResourceIsolationIds) {
      useTokenStore(id).clearAll()
      useCollectionStore(id).clearSelection()
    }
    const reload = useCollectionReloadStore('mainId')
    reload.requestMainCollectionReload()
    reload.requestSubCollectionReload()
    await Promise.all((['detailId', 'subDetailId'] as const).map(async (id) => {
      const resource = useRouteResourceStore(id)
      if (resource.requestedId !== null) await resource.load(resource.requestedId, true)
    }))
    useMessageStore('mainId').success(audience === 'public' ? 'Photos published.' : 'Photos made private.')
  })
}
