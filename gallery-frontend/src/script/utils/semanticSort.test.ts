import { describe, expect, it } from 'vitest'
import { hasSemanticSearch, nextQuickSortOrder, parseGallerySortOrder } from './gallerySort'
import { canSaveSearch } from '@/components/SavedSearch/savedSearchRoute'

describe('semantic sort controls', () => {
  it('recognizes semantic syntax without confusing text inside another filter', () => {
    expect(hasSemanticSearch('and(tag:"family", semantic:"enfant à vélo")')).toBe(true)
    expect(hasSemanticSearch('tag:"semantic:hello"')).toBe(false)
    expect(hasSemanticSearch(null)).toBe(false)
    expect(parseGallerySortOrder('relevance')).toBe('relevance')
    expect(canSaveSearch('home', 'semantic:"bird"')).toBe(false)
    expect(canSaveSearch('home', 'tag:"semantic:hello"')).toBe(true)
  })

  it('cycles through date orders back to relevance only for semantic searches', () => {
    expect(nextQuickSortOrder('relevance', true)).toBe('descending')
    expect(nextQuickSortOrder('descending', true)).toBe('ascending')
    expect(nextQuickSortOrder('ascending', true)).toBe('relevance')
    expect(nextQuickSortOrder('ascending')).toBe('descending')
  })
})
