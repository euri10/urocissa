import axios from 'axios'
import { z } from 'zod'

const statusSchema = z.object({
  online: z.boolean(),
  offlineSince: z.number().nullable(),
  modelId: z.string().nullable(),
  device: z.string().nullable(),
  totalPhotos: z.number().int().nonnegative(),
  embeddedPhotos: z.number().int().nonnegative(),
  pendingPhotos: z.number().int().nonnegative(),
  retryingPhotos: z.number().int().nonnegative(),
  backfillPaused: z.boolean(),
  processing: z.boolean(),
  lastError: z.string().nullable()
})

export type EmbeddingStatus = z.infer<typeof statusSchema>

export async function getEmbeddingStatus(signal?: AbortSignal): Promise<EmbeddingStatus> {
  const response = await axios.get('/get/embedding-status', { signal, timeout: 10000 })
  return statusSchema.parse(response.data)
}

export async function pauseEmbeddingBackfill(
  paused: boolean,
  signal?: AbortSignal
): Promise<EmbeddingStatus> {
  const response = await axios.put('/put/embedding-backfill', { paused }, { signal, timeout: 10000 })
  return statusSchema.parse(response.data)
}
