import { describe, expect, it } from 'vitest'
import { GRID_TIER_LONG_SIDE, chooseThumbnailTier, getThumbnailSrc } from './thumbnail'

const hash = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'

describe('getThumbnailSrc', () => {
  it('keeps the legacy filename for version zero', () => {
    expect(getThumbnailSrc(hash, 0)).toBe(`/object/compressed/01/${hash}.jpg`)
  })

  it('uses an immutable versioned filename after replacement', () => {
    expect(getThumbnailSrc(hash, 42)).toBe(`/object/compressed/01/${hash}-v42.jpg`)
  })

  it('defaults to the viewer tier so the fullscreen viewer is unchanged', () => {
    expect(getThumbnailSrc(hash, 0)).toBe(getThumbnailSrc(hash, 0, 'compressed'))
  })

  it('addresses the grid tier under its own directory', () => {
    expect(getThumbnailSrc(hash, 0, 'thumbnail')).toBe(`/object/thumbnail/01/${hash}.jpg`)
    expect(getThumbnailSrc(hash, 42, 'thumbnail')).toBe(`/object/thumbnail/01/${hash}-v42.jpg`)
  })
})

describe('chooseThumbnailTier', () => {
  it('uses the grid tier for a default row at device pixel ratio 1', () => {
    // subRowHeightScale defaults to 250; a 3:2 tile is then 375x250.
    expect(chooseThumbnailTier(375, 250, 1, false)).toBe('thumbnail')
  })

  it('uses the grid tier for a default row at device pixel ratio 2', () => {
    expect(chooseThumbnailTier(375, 250, 2, false)).toBe('thumbnail')
  })

  it('uses the grid tier at the maximum row height and ratio 1', () => {
    // subRowHeightScale is clamped to 450 by the settings modal.
    expect(chooseThumbnailTier(675, 450, 1, false)).toBe('thumbnail')
  })

  it('falls back to the viewer tier when a tile needs more pixels', () => {
    expect(chooseThumbnailTier(675, 450, 2, false)).toBe('compressed')
    expect(chooseThumbnailTier(1000, 250, 1, false)).toBe('compressed')
  })

  it('switches tiers exactly at the grid long side', () => {
    expect(chooseThumbnailTier(GRID_TIER_LONG_SIDE, 250, 1, false)).toBe('thumbnail')
    expect(chooseThumbnailTier(GRID_TIER_LONG_SIDE + 1, 250, 1, false)).toBe('compressed')
  })

  it('treats a sub-unit device pixel ratio as 1 rather than shrinking further', () => {
    expect(chooseThumbnailTier(GRID_TIER_LONG_SIDE + 1, 250, 0.5, false)).toBe('compressed')
  })

  it('uses the viewer tier for album covers, whose cover scale needs the aspect ratio', () => {
    expect(chooseThumbnailTier(250, 250, 1, true)).toBe('compressed')
  })
})
