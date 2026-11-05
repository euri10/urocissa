import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { fetchDataInWorker } from '@/api/fetchData'
import { fetchRowInWorker } from '@/api/fetchRow'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useTokenStore } from '@/store/tokenStore'
import { useShareStore } from '@/store/shareStore'
import { fromDataWorker, toDataWorker } from './workerApi'
import type { ReturnDataPayload } from './workerApi'
import { createHandler } from 'typesafe-agent-events'

const transport = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  worker: { postMessage: vi.fn() }
}))
vi.mock('axios', () => ({ default: {
  create: () => ({
    get: transport.get,
    interceptors: { response: { use: vi.fn() } }
  })
} }))
vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))
vi.mock('@/store/workerStore', () => ({
  useWorkerStore: () => ({ worker: transport.worker })
}))

type Action = ReturnType<(typeof toDataWorker)[keyof typeof toDataWorker]>

describe('share authorization across the data-worker boundary', () => {
  beforeEach(() => {
    vi.resetModules()
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })
  afterEach(() => vi.unstubAllGlobals())

  it.each(['data', 'row'] as const)('forwards current share credentials for %s requests', async (kind) => {
    let receive!: (event: MessageEvent<Action>) => void
    vi.stubGlobal('self', {
      postMessage: transport.post,
      addEventListener: (_name: string, handler: typeof receive) => { receive = handler }
    })
    await import('./toDataWorker')
    transport.worker.postMessage.mockImplementation((action: Action) => {
      receive({ data: action } as MessageEvent<Action>)
    })
    const prefetch = usePrefetchStore('mainId')
    const token = useTokenStore('mainId')
    vi.spyOn(token, 'refreshTimestampTokenIfExpired').mockResolvedValue()
    const share = useShareStore('mainId')
    for (const [index, context] of [
      { albumId: 'album-a', shareId: 'share-a', password: 'test' },
      { albumId: 'album-b', shareId: 'share-b', password: null },
      { albumId: null, shareId: null, password: null }
    ].entries()) {
      Object.assign(share, context)
      prefetch.timestamp = 100 + index
      prefetch.dataLength = 1
      prefetch.rowLength = 3
      prefetch.windowWidth = 1000
      token.timestampToken = `fixture-token-${index}`
      transport.get.mockResolvedValueOnce({ data: kind === 'data' ? [] : {
        start: 0, end: 1, rowIndex: index, displayElements: [{ displayWidth: 640, displayHeight: 480 }]
      } })
      if (kind === 'data') await fetchDataInWorker('single', 0, 'mainId')
      else await fetchRowInWorker(index, 'mainId')
      const expected: Record<string, string> = { Authorization: `Bearer fixture-token-${index}` }
      if (context.albumId !== null) {
        expected['x-album-id'] = context.albumId
        expected['x-share-id'] = context.shareId
        if (context.password !== null) expected['x-share-password'] = context.password
      }
      await vi.waitFor(() => {
        expect(transport.get).toHaveBeenNthCalledWith(
          index + 1,
          expect.stringContaining(kind === 'data' ? '/get/get-data?' : '/get/get-rows?'),
          { headers: expected }
        )
      })
    }
  })

  it.each(['private', 'public', undefined] as const)('preserves audience=%s through the real worker response parser', async (audience) => {
    let receive!: (event: MessageEvent<Action>) => void
    vi.stubGlobal('self', {
      postMessage: transport.post,
      addEventListener: (_name: string, handler: typeof receive) => { receive = handler }
    })
    await import('./toDataWorker')
    const returned = vi.fn<(payload: ReturnDataPayload) => void>()
    const receiveReturn = createHandler<typeof fromDataWorker>({
      returnData: returned,
      fetchRowReturn: vi.fn(),
      editTagsReturn: vi.fn(),
      notification: vi.fn(),
      unauthorized: vi.fn(),
      refreshTimestampToken: vi.fn(),
      refreshHashToken: vi.fn()
    })
    transport.post.mockImplementation((action: ReturnType<typeof fromDataWorker.returnData>) => {
      receiveReturn(action)
    })
    transport.get.mockResolvedValueOnce({ data: [{
      abstractData: {
        type: 'image', id: 'fixture-photo', pending: false,
        width: 100, height: 80, ext: 'jpg', size: 42
      },
      timestamp: 100,
      token: 'fixture-hash-token',
      ...(audience === undefined ? {} : { audience })
    }] })
    receive({ data: toDataWorker.fetchData({
      fetchMethod: 'single', batch: 0, timestamp: 100,
      timestampToken: 'fixture-timestamp-token', shareHeaders: {}
    }) } as MessageEvent<Action>)
    await vi.waitFor(() => { expect(returned).toHaveBeenCalledOnce() })
    const media = returned.mock.calls[0]?.[0].slicedDataArray[0]?.data
    expect(media).toMatchObject({ type: 'image', id: 'fixture-photo', timestamp: 100 })
    if (audience === undefined) expect(media).not.toHaveProperty('audience')
    else expect(media).toHaveProperty('audience', audience)
  })
})
