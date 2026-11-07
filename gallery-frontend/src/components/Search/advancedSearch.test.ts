import { describe, expect, it } from 'vitest'
import { generateJsonString } from '@/script/lexer/generateJson'
import {
  buildAdvancedSearchFilter,
  createEmptyAdvancedSearchCriteria,
  type AdvancedSearchCriteria
} from './advancedSearch'

function criteria(overrides: Partial<AdvancedSearchCriteria> = {}): AdvancedSearchCriteria {
  return { ...createEmptyAdvancedSearchCriteria(), ...overrides }
}

describe('advanced search filter builder', () => {
  it('builds multilingual semantic search with existing filters and escaped values', () => {
    for (const text of ['enfant à vélo', 'child on a bike', 'a "sunset" near C:\\photos']) {
      const query = buildAdvancedSearchFilter(criteria({ semantic: text, tag: 'family', audience: 'private' }))
      expect(JSON.parse(generateJsonString(query))).toEqual({
        And: [{ Semantic: text }, { Tag: 'family' }, { Audience: 'private' }]
      })
    }
    expect(buildAdvancedSearchFilter(criteria({ semantic: null }))).toBe('')
  })

  it('returns an empty filter when every criterion is empty or all', () => {
    expect(buildAdvancedSearchFilter(criteria())).toBe('')
    expect(createEmptyAdvancedSearchCriteria().sortOrder).toBe('descending')
    expect(createEmptyAdvancedSearchCriteria('random').sortOrder).toBe('random')
  })

  it('returns a single atomic expression without an and wrapper', () => {
    expect(buildAdvancedSearchFilter(criteria({ filename: '  vacation/photo.jpg  ' }))).toBe(
      'path:"vacation/photo.jpg"'
    )
  })

  it('trims and escapes quotes and backslashes', () => {
    expect(buildAdvancedSearchFilter(criteria({ keyword: '  C:\\photos "summer"  ' }))).toBe(
      'any:"C:\\\\photos \\"summer\\""'
    )
  })

  it('combines populated fields and includes the album media type', () => {
    expect(
      buildAdvancedSearchFilter(
        criteria({
          keyword: 'sunset',
          tag: 'family',
          extension: 'jpg',
          cameraMake: 'Canon',
          cameraModel: 'R5',
          mediaType: 'album',
          sortOrder: 'ascending'
        })
      )
    ).toBe(
      'and(any:"sunset", tag:"family", ext:"jpg", make:"Canon", model:"R5", type:"album")'
    )
  })

  it('handles nullable camera fields and trims selected or manually entered values', () => {
    expect(
      buildAdvancedSearchFilter(
        criteria({ cameraMake: '  Canon  ', cameraModel: null, mediaType: 'image' })
      )
    ).toBe('and(make:"Canon", type:"image")')
  })

  it.each(['private', 'public'] as const)('combines %s audience with tags and media type', (audience) => {
    const query = buildAdvancedSearchFilter(criteria({ audience, tag: 'family', mediaType: 'image' }))
    expect(query).toBe(`and(tag:"family", type:"image", audience:"${audience}")`)
    expect(JSON.parse(generateJsonString(query))).toEqual({
      And: [{ Tag: 'family' }, { ExtType: 'image' }, { Audience: audience }]
    })
    expect(createEmptyAdvancedSearchCriteria().audience).toBe('all')
  })
})
