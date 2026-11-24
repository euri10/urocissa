import { describe, expect, it } from 'vitest'
import { photoIds, summary, type ReviewPhoto } from './review'

const photo = (judgments: ('yes' | 'no' | 'uncertain' | null)[]): ReviewPhoto => ({
  hash: 'a'.repeat(64), version: 0, timestamp: 1, role: 'random', enabled: true, pending: false,
  before: judgments.map((judgment, i) => ({ concept: String(i), label: String(i), judgment })),
  after: null, seenBefore: true, seenAfter: false
})

describe('evaluation is independent of tag feedback', () => {
  it('does not turn empty or uncertain labels into a precision pass', () => {
    expect(summary(Array.from({ length: 100 }, () => photo([]))).precision).toBeNull()
    expect(summary(Array.from({ length: 100 }, () => photo([]))).pass).toBe(false)
    const result = summary(Array.from({ length: 100 }, () => photo(['yes', 'uncertain'])))
    expect(result.precision).toBe(1)
    expect(result.lowerBound).toBe(.5)
    expect(result.pass).toBe(false)
  })
  it('requires all 100 photos and counts coverage separately', () => {
    const photos = Array.from({ length: 100 }, (_, i) => photo(i < 85 ? ['yes'] : ['no']))
    expect(summary(photos).pass).toBe(true)
    expect(summary(photos.slice(0, 20)).pass).toBe(false)
    const first = photos[0]
    if (first) first.seenBefore = false
    expect(summary(photos).pass).toBe(false)
    expect(summary(photos).covered).toBe(100)
  })
  it('keeps before and after judgments separate and strips link queries', () => {
    const item = photo(['no'])
    item.after = [{ concept: 'plant', label: 'plant', judgment: 'yes' }]
    item.seenAfter = true
    expect(summary([item]).no).toBe(1)
    expect(summary([item], true).yes).toBe(1)
    const id = 'b'.repeat(64)
    expect(photoIds(`https://photos.sslc.rocks/all/view/${id}?sort=random`)).toEqual([id])
    expect(photoIds(`https://other.invalid/all/view/${id}`)).toEqual([])
  })
})
