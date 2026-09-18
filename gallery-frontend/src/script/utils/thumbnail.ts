/** Derived-image tiers the backend publishes per item. */
export type ThumbnailTier = 'compressed' | 'thumbnail'

/**
 * Long side of the `thumbnail` tier, matching `GRID_LONG_SIDE` in
 * gallery-backend `operations/indexation/generate_thumbnail.rs`.
 */
export const GRID_TIER_LONG_SIDE = 768

export function getThumbnailSrc(
  hash: string,
  cacheVersion: number,
  tier: ThumbnailTier = 'compressed'
): string {
  const fileName = cacheVersion === 0 ? `${hash}.jpg` : `${hash}-v${cacheVersion}.jpg`
  return `/object/${tier}/${hash.slice(0, 2)}/${fileName}`
}

/**
 * Pick the smallest tier that still covers the pixels a tile will paint.
 *
 * The tier has to be chosen before the fetch, so album covers always use
 * `compressed`: cover scaling needs source pixels proportional to the image's
 * aspect ratio, which is not known until the blob is decoded. Ordinary grid
 * tiles are bounded by their laid-out box, so the comparison is exact.
 */
export function chooseThumbnailTier(
  displayWidth: number,
  displayHeight: number,
  devicePixelRatio: number,
  albumMode: boolean
): ThumbnailTier {
  if (albumMode) return 'compressed'
  const ratio = Math.max(devicePixelRatio, 1)
  const required = Math.max(displayWidth, displayHeight) * ratio
  return required <= GRID_TIER_LONG_SIDE ? 'thumbnail' : 'compressed'
}
