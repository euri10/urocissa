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
import { useSearchFacetStore } from '@/store/searchFacetStore'
import { z } from 'zod'

export type AutoTagAction = 'confirm' | 'reject'
const previewSchema = z.object({
  photos: z.number().int().nonnegative(),
  publicPhotos: z.number().int().nonnegative(),
  concepts: z.array(z.object({ concept: z.string(), label: z.string() }))
})
export type AutoTagSelection = z.infer<typeof previewSchema>

function decisionTarget(target: string | SelectionInput, isolationId: IsolationId) {
  if (typeof target === 'string') return { hash: target }
  const prefetch = usePrefetchStore(isolationId)
  if (prefetch.timestamp === null) throw new Error('The collection is not ready.')
  return { selection: normalizeSelection(target), timestamp: prefetch.timestamp }
}

export async function previewAutoTagSelection(
  target: string | SelectionInput, isolationId: IsolationId
): Promise<AutoTagSelection> {
  if (!useConfigStore('mainId').isAdmin || isolationId === 'shareId') throw new Error('Owner access is required.')
  const response = await axios.post('/post/auto_tag_selection', decisionTarget(target, isolationId))
  return previewSchema.parse(response.data)
}

export async function decideAutoTag(
  target: string | SelectionInput, concept: string, action: AutoTagAction, isolationId: IsolationId
): Promise<boolean> {
  const config = useConfigStore('mainId')
  if (!config.isAdmin || !config.canWrite || isolationId === 'shareId') return false
  const payload = decisionTarget(target, isolationId)
  const ids = typeof target === 'string' ? [target]
    : selectedCachedResourceIds(isolationId, normalizeSelection(target))
  const response = await axios.put('/put/auto_tag_decision', { ...payload, concept, action })
  const result = z.object({ photos: z.number().int().nonnegative(), label: z.string() }).parse(response.data)
  for (const id of ids) updateCachedResource(id, (data, isolation) => {
    if (data.type !== 'image' || isolation === 'shareId') return
    if (action === 'confirm' && !data.tags.includes(result.label)) data.tags = [...data.tags, result.label]
    data.autoTags = (data.autoTags ?? []).filter(tag => tag.concept !== concept)
    data.autoTagsPending = data.autoTagsEnabled !== false
  })
  const reload = useCollectionReloadStore('mainId')
  reload.requestMainCollectionReload()
  reload.requestSubCollectionReload()
  const facets = useSearchFacetStore()
  facets.fetched = false
  await facets.fetchFacets()
  useMessageStore('mainId').success(`${action === 'confirm' ? 'Confirmed' : 'Rejected'} ${result.label} for ${result.photos} photos.`)
  return true
}

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
