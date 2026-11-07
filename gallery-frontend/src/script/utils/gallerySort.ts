import type { GallerySortOrder } from '@/type/types'
import { MyLexer } from '@/script/lexer/lexer'

export interface GallerySearchSubmission {
  query: string
  sortOrder: GallerySortOrder
}

export interface QuickSortPresentation {
  icon: string
  ariaLabel: string
}

export function parseGallerySortOrder(value: unknown): GallerySortOrder {
  return value === 'ascending' || value === 'random' || value === 'relevance' ? value : 'descending'
}

export function hasSemanticSearch(query: string | null | undefined): boolean {
  return MyLexer.tokenize(query ?? '').tokens.some((token) => token.tokenType.name === 'Semantic')
}

export function nextQuickSortOrder(sortOrder: GallerySortOrder, semantic = false): GallerySortOrder {
  if (semantic && sortOrder === 'ascending') return 'relevance'
  return sortOrder === 'descending' ? 'ascending' : 'descending'
}

export function getQuickSortPresentation(
  sortOrder: GallerySortOrder
): QuickSortPresentation {
  switch (sortOrder) {
    case 'relevance':
      return {
        icon: 'mdi-sort-variant',
        ariaLabel: 'Currently sorted by relevance. Switch to descending.'
      }
    case 'ascending':
      return {
        icon: 'mdi-sort-ascending',
        ariaLabel: 'Currently sorted ascending. Switch to descending.'
      }
    case 'random':
      return {
        icon: 'mdi-shuffle',
        ariaLabel: 'Currently sorted randomly. Switch to descending.'
      }
    case 'descending':
      return {
        icon: 'mdi-sort-descending',
        ariaLabel: 'Currently sorted descending. Switch to ascending.'
    }
  }
}
