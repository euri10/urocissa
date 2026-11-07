import { escapeAndWrap } from '@utils/escape'
import type { GallerySortOrder } from '@/type/types'

export type AdvancedSearchMediaType = 'all' | 'image' | 'video' | 'album'

export interface AdvancedSearchCriteria {
  keyword: string
  semantic: string | null
  filename: string
  tag: string | null
  extension: string
  cameraMake: string | null
  cameraModel: string | null
  mediaType: AdvancedSearchMediaType
  audience: 'all' | 'private' | 'public'
  sortOrder: GallerySortOrder
}

export function createEmptyAdvancedSearchCriteria(
  sortOrder: GallerySortOrder = 'descending'
): AdvancedSearchCriteria {
  return {
    keyword: '',
    semantic: '',
    filename: '',
    tag: null,
    extension: '',
    cameraMake: null,
    cameraModel: null,
    mediaType: 'all',
    audience: 'all',
    sortOrder
  }
}

export function buildAdvancedSearchFilter(criteria: AdvancedSearchCriteria): string {
  const parts: string[] = []

  const semantic = criteria.semantic?.trim() ?? ''
  if (semantic !== '') parts.push(`semantic:${escapeAndWrap(semantic)}`)

  const keyword = criteria.keyword.trim()
  if (keyword !== '') parts.push(`any:${escapeAndWrap(keyword)}`)

  const filename = criteria.filename.trim()
  if (filename !== '') parts.push(`path:${escapeAndWrap(filename)}`)

  const tag = criteria.tag?.trim() ?? ''
  if (tag !== '') parts.push(`tag:${escapeAndWrap(tag)}`)

  const extension = criteria.extension.trim()
  if (extension !== '') parts.push(`ext:${escapeAndWrap(extension)}`)

  const cameraMake = criteria.cameraMake?.trim() ?? ''
  if (cameraMake !== '') parts.push(`make:${escapeAndWrap(cameraMake)}`)

  const cameraModel = criteria.cameraModel?.trim() ?? ''
  if (cameraModel !== '') parts.push(`model:${escapeAndWrap(cameraModel)}`)

  if (criteria.mediaType !== 'all') {
    parts.push(`type:${escapeAndWrap(criteria.mediaType)}`)
  }

  if (criteria.audience !== 'all') {
    parts.push(`audience:${escapeAndWrap(criteria.audience)}`)
  }

  if (parts.length === 0) return ''
  if (parts.length === 1) return parts[0] ?? ''
  return `and(${parts.join(', ')})`
}
