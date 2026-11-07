import axios from 'axios'
import { afterEach, expect, it, vi } from 'vitest'
import { getEmbeddingStatus, pauseEmbeddingBackfill, type EmbeddingStatus } from './embeddings'

const status: EmbeddingStatus = {
  online: false,
  offlineSince: 123,
  modelId: null,
  device: null,
  totalPhotos: 2,
  embeddedPhotos: 0,
  pendingPhotos: 2,
  retryingPhotos: 0,
  backfillPaused: false,
  processing: false,
  lastError: 'Tagger offline'
}

afterEach(() => vi.restoreAllMocks())

it('validates owner status and bounds requests with cancellation and a timeout', async () => {
  const get = vi.spyOn(axios, 'get').mockResolvedValue({ data: status })
  const signal = new AbortController().signal
  expect(await getEmbeddingStatus(signal)).toEqual(status)
  expect(get).toHaveBeenCalledWith('/get/embedding-status', { signal, timeout: 10000 })
  get.mockResolvedValue({ data: { ...status, pendingPhotos: -1 } })
  await expect(getEmbeddingStatus()).rejects.toThrow()
})

it('sends an explicit pause value and returns server state', async () => {
  const paused = { ...status, backfillPaused: true }
  const put = vi.spyOn(axios, 'put').mockResolvedValue({ data: paused })
  expect(await pauseEmbeddingBackfill(true)).toEqual(paused)
  expect(put).toHaveBeenCalledWith('/put/embedding-backfill', { paused: true }, { timeout: 10000, signal: undefined })
})

it('propagates status failures so the panel can show its retry state', async () => {
  vi.spyOn(axios, 'get').mockRejectedValue(new Error('offline'))
  await expect(getEmbeddingStatus()).rejects.toThrow('offline')
})
