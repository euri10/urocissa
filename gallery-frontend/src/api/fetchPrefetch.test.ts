import { afterEach, describe, expect, it, vi } from 'vitest'
import axios from 'axios'
import { prefetch } from './fetchPrefetch'

describe('prefetch API', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('preserves the owner-only review summary and sends its explicit Concept parameter', async () => {
    const summary = { concept: 'cat', label: 'cat', positiveExamples: 4, negativeExamples: 5 }
    const post = vi.spyOn(axios, 'post').mockResolvedValue({ data: { prefetch: { timestamp: 3, dataLength: 2, locateTo: null }, token: 'token', resolvedShareOpt: null, conceptReview: summary } })
    const result = await prefetch(null, '', 'descending', null, undefined, 'cat')
    expect(post.mock.calls[0]?.[2]?.params).toMatchObject({ review_concept: 'cat' })
    expect(result.conceptReview).toEqual(summary)
  })

  it('sends the typed sort order and lets axios encode locate', async () => {
    const post = vi.spyOn(axios, 'post').mockResolvedValue({
      data: {
        prefetch: {
          timestamp: 1,
          dataLength: 0,
          locateTo: null
        },
        token: 'token',
        resolvedShareOpt: null
      }
    })

    await prefetch('{"tag":"family"}', 'default', 'ascending', 'photo / 1')

    expect(post).toHaveBeenCalledWith('/get/prefetch', '{"tag":"family"}', {
      params: {
        locate: 'photo / 1',
        sort: 'ascending'
      },
      headers: {
        'Content-Type': 'application/json'
      }
    })
  })

  it('uses descending by default and omits a null locate value', async () => {
    const post = vi.spyOn(axios, 'post').mockResolvedValue({
      data: {
        prefetch: {
          timestamp: 2,
          dataLength: 0,
          locateTo: null
        },
        token: 'token',
        resolvedShareOpt: null
      }
    })

    await prefetch(null)

    expect(post).toHaveBeenCalledWith('/get/prefetch', null, {
      params: {
        locate: undefined,
        sort: 'descending'
      },
      headers: {
        'Content-Type': 'application/json'
      }
    })
  })
})
