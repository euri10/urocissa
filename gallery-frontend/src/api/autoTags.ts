import axios from 'axios'
import { useConfigStore } from '@/store/configStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { useRouteResourceStore } from '@/store/routeResourceStore'
import { useMessageStore } from '@/store/messageStore'
import { tryWithMessageStore } from '@/script/utils/try_catch'
import { selectedCachedResourceIds, updateCachedResource } from '@/script/utils/routeResourceCache'
import { normalizeSelection, selectionCount, type SelectionInput } from '@/type/selection'
import type { IsolationId } from '@/type/types'
import { autoTagStatusSchema } from '@/type/schemas'

export async function fetchAutoTags(id: string, signal: AbortSignal): Promise<boolean> {
  const response = await axios.get(`/get/auto-tags/${encodeURIComponent(id)}`, { signal, timeout: 5000 })
  const status = autoTagStatusSchema.parse(response.data)
  updateCachedResource(id, (data, isolation) => {
    if (data.type === 'image' && isolation !== 'shareId') Object.assign(data, status)
  })
  return status.autoTagsPending
}

export async function editAutoTags(
  target: string | SelectionInput,
  enabled: boolean,
  isolationId: IsolationId
): Promise<void> {
  if (!useConfigStore('mainId').canWrite) return
  const prefetch = usePrefetchStore(isolationId)
  const selection = typeof target === 'string' ? undefined : normalizeSelection(target)
  const count = selection === undefined ? 1 : selectionCount(selection, prefetch.dataLength)
  if (count === 0) return
  if (selection !== undefined && prefetch.timestamp === null) {
    useMessageStore('mainId').error('Cannot update Auto tags because the collection is not ready.')
    return
  }
  const timestamp = prefetch.timestamp
  const ids = new Set(typeof target === 'string' ? [target] : selectedCachedResourceIds(isolationId, normalizeSelection(target)))
  await tryWithMessageStore('mainId', async () => {
    await axios.put('/put/edit_auto_tags', typeof target === 'string'
      ? { hash: target, enabled } : { selection, timestamp, enabled })
    if (selection !== undefined && prefetch.timestamp === timestamp) {
      for (const id of selectedCachedResourceIds(isolationId, selection)) ids.add(id)
    }
    for (const id of ids) updateCachedResource(id, (data, isolation) => {
      if (data.type === 'image' && isolation !== 'shareId') {
        data.autoTagsEnabled = enabled
        data.autoTagsPending = enabled
        data.autoTags = []
      }
    })
    if (ids.size < count) {
      const reload = useCollectionReloadStore('mainId')
      reload.requestMainCollectionReload()
      reload.requestSubCollectionReload()
      await Promise.all((['detailId', 'subDetailId'] as const).map(async (isolation) => {
        const resource = useRouteResourceStore(isolation)
        if (resource.requestedId !== null) await resource.load(resource.requestedId, true)
      }))
    }
    useMessageStore('mainId').success(enabled ? 'Auto tags turned on for photos.' : 'Auto tags turned off for photos.')
  })
}
