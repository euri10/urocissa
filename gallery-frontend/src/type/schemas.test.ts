import { describe, expect, it } from 'vitest'
import {
  BackendDataParser,
  PublicConfigSchema,
  savedSearchListSchema,
  searchFacetsSchema
} from './schemas'

const config = {
  address: '127.0.0.1',
  port: 5673,
  limits: { json: '10MiB' },
  syncPaths: [],
  readOnlyMode: false,
  disableImg: false,
  writeBehind: {
    flushIntervalMs: 1_000,
    softLimitMiB: 16,
    hardLimitMiB: 32
  }
}

describe('public write-behind config schema', () => {
  it('accepts the defaults', () => {
    expect(PublicConfigSchema.parse(config).writeBehind).toEqual(config.writeBehind)
  })

  it('rejects invalid interval and memory limits', () => {
    expect(() =>
      PublicConfigSchema.parse({
        ...config,
        writeBehind: { flushIntervalMs: 99, softLimitMiB: 16, hardLimitMiB: 16 }
      })
    ).toThrow()
    expect(() =>
      PublicConfigSchema.parse({
        ...config,
        writeBehind: { flushIntervalMs: 1_000, softLimitMiB: 32, hardLimitMiB: 257 }
      })
    ).toThrow()
  })
})

describe('thumbnail cacheVersion schema', () => {
  const image = {
    type: 'image',
    id: 'image',
    pending: false,
    width: 100,
    height: 80,
    ext: 'jpg',
    size: 42
  }

  it('defaults old API payloads to version zero', () => {
    expect(BackendDataParser.parse(image).cacheVersion).toBe(0)
  })

  it('preserves a mirrored album cover version', () => {
    const album = BackendDataParser.parse({
      type: 'album',
      id: 'album',
      pending: false,
      title: null,
      startTime: null,
      endTime: null,
      lastModifiedTime: 1,
      cover: 'image',
      itemCount: 1,
      itemSize: 42,
      cacheVersion: 7
    })
    if (album.type !== 'album') throw new Error('expected album')
    expect(album.cover).toBe('image')
    expect(album.cacheVersion).toBe(7)
  })

  it('only accepts checked u32 values', () => {
    for (const cacheVersion of [-1, 1.5, 0x1_0000_0000]) {
      expect(BackendDataParser.safeParse({ ...image, cacheVersion }).success).toBe(false)
    }
  })
})

describe('public metadata records', () => {
  it.each(['image', 'video'])('accepts a redacted %s without private fields', (type) => {
    const media = BackendDataParser.parse({
      type, id: 'public-media', pending: false, width: 1920, height: 1080, ext: 'jpg',
      size: 1000, exifVec: { Make: 'Camera' }, cacheVersion: 7
    })
    if (media.type === 'album') throw new Error('expected media')
    expect(media.alias).toEqual([])
    expect(media.updateAt).toBe(0)
    expect(media.width).toBe(1920)
    expect(media.exif).toEqual({ Make: 'Camera' })
    expect(media.cacheVersion).toBe(7)
  })

  it('accepts an album with hidden date range and share credentials', () => {
    const album = BackendDataParser.parse({
      type: 'album', id: 'public-album', pending: false, title: 'Album', cover: null,
      startTime: null, endTime: null, lastModifiedTime: Date.UTC(2026, 5, 1),
      itemCount: 2, itemSize: 1000
    })
    if (album.type !== 'album') throw new Error('expected album')
    expect(album.startTime).toBeNull()
    expect(album.endTime).toBeNull()
    expect(album.shareList).toEqual({})
  })
})

describe('photo audience metadata', () => {
  it.each(['image', 'video'])('preserves explicit owner audiences on %s', (type) => {
    const media = { type, id: 'photo', pending: false, width: 100, height: 80, ext: 'jpg', size: 42 }
    for (const audience of ['private', 'public']) {
      expect(BackendDataParser.parse({ ...media, audience })).toHaveProperty('audience', audience)
    }
    expect(BackendDataParser.parse(media)).not.toHaveProperty('audience')
    expect(BackendDataParser.safeParse({ ...media, audience: 'unknown' }).success).toBe(false)
  })

  it('does not introduce photo audience metadata on albums', () => {
    expect(BackendDataParser.parse({
      type: 'album', id: 'album', pending: false, title: null, startTime: null,
      endTime: null, lastModifiedTime: 1, cover: null, itemCount: 0, itemSize: 0,
      audience: 'private'
    })).not.toHaveProperty('audience')
  })
})

describe('search facets schema', () => {
  it('accepts independent tag, make, and model arrays', () => {
    expect(
      searchFacetsSchema.parse({
        tags: [{ value: 'family', count: 2 }],
        makes: [{ value: 'Canon', count: 3 }],
        models: [{ value: 'R5', count: 1 }]
      })
    ).toEqual({
      tags: [{ value: 'family', count: 2 }],
      makes: [{ value: 'Canon', count: 3 }],
      models: [{ value: 'R5', count: 1 }]
    })
  })

  it('rejects missing facets and invalid counts', () => {
    expect(
      searchFacetsSchema.safeParse({ tags: [], makes: [{ value: 'Canon', count: -1 }] }).success
    ).toBe(false)
  })
})

describe('saved search schema', () => {
  const savedSearch = {
    id: '020b6f4f-5c28-4f8c-81f8-bc22949f1ee8',
    name: ' Family ',
    context: 'favorite',
    query: ' tag:family '
  }

  it('parses and trims a valid saved search response', () => {
    expect(savedSearchListSchema.parse([savedSearch])).toEqual([
      {
        ...savedSearch,
        name: 'Family',
        query: 'tag:family',
        sortOrder: 'descending'
      }
    ])
    expect(
      savedSearchListSchema.safeParse([
        { ...savedSearch, name: '📷'.repeat(80), query: '🔎'.repeat(4096) }
      ]).success
    ).toBe(true)
  })

  it('rejects invalid contexts, empty values, and more than 50 entries', () => {
    expect(savedSearchListSchema.safeParse([{ ...savedSearch, context: 'links' }]).success).toBe(
      false
    )
    expect(savedSearchListSchema.safeParse([{ ...savedSearch, name: ' ' }]).success).toBe(false)
    expect(
      savedSearchListSchema.safeParse([{ ...savedSearch, sortOrder: 'newest' }]).success
    ).toBe(false)
    expect(
      savedSearchListSchema.safeParse([{ ...savedSearch, name: '📷'.repeat(81) }]).success
    ).toBe(false)
    expect(
      savedSearchListSchema.safeParse([{ ...savedSearch, query: '🔎'.repeat(4097) }]).success
    ).toBe(false)
    expect(savedSearchListSchema.safeParse(Array.from({ length: 51 }, () => savedSearch)).success).toBe(
      false
    )
  })
})
