import type { ThumbnailTier } from '@utils/thumbnail'

export const THUMBNAIL_CACHE_NAME = 'img-blob-cache-v3'

/** Superseded cache names. v2 keys omit the tier, so its entries are unreachable. */
export const LEGACY_THUMBNAIL_CACHE_NAMES = ['img-blob-cache-v1', 'img-blob-cache-v2']

export type ThumbnailDiskCache = Pick<Cache, 'delete' | 'keys' | 'match' | 'put'>

export interface ThumbnailCacheStorage {
  delete(cacheName: string): Promise<boolean>
  open(cacheName: string): Promise<ThumbnailDiskCache>
}

export function thumbnailIdentity(
  hash: string,
  cacheVersion: number,
  tier: ThumbnailTier = 'compressed'
): string {
  return `${tier}:${hash}:v${cacheVersion}`
}

export function thumbnailCacheKey(
  hash: string,
  cacheVersion: number,
  tier: ThumbnailTier = 'compressed'
): string {
  return `https://img-blob-cache.internal/${tier}/${hash}/v${cacheVersion}`
}

function versionFromCacheUrl(url: string, hash: string, tier: ThumbnailTier): number | undefined {
  const pathPrefix = `/${tier}/${hash}/v`
  const pathname = new URL(url).pathname
  if (!pathname.startsWith(pathPrefix)) return undefined
  const version = Number(pathname.slice(pathPrefix.length))
  return Number.isSafeInteger(version) && version >= 0 ? version : undefined
}

/** Worker-owned two-layer thumbnail Blob cache. */
export class ThumbnailBlobCache {
  private readonly memory = new Map<string, Blob>()
  private pendingDiskWrite: Promise<void> = Promise.resolve()

  constructor(
    private readonly storage: ThumbnailCacheStorage | undefined =
      typeof caches === 'undefined' ? undefined : caches
  ) {}

  async cleanupLegacy(): Promise<void> {
    if (this.storage === undefined) return
    for (const name of LEGACY_THUMBNAIL_CACHE_NAMES) {
      try {
        await this.storage.delete(name)
      } catch {
        // Cache API is an optional optimization.
      }
    }
  }

  async get(
    hash: string,
    cacheVersion: number,
    tier: ThumbnailTier = 'compressed'
  ): Promise<Blob | undefined> {
    const identity = thumbnailIdentity(hash, cacheVersion, tier)
    const memoryBlob = this.memory.get(identity)
    if (memoryBlob !== undefined) return memoryBlob
    if (this.storage === undefined) return undefined

    try {
      const cache = await this.storage.open(THUMBNAIL_CACHE_NAME)
      const key = thumbnailCacheKey(hash, cacheVersion, tier)
      const response = await cache.match(key)
      if (response === undefined) return undefined
      const blob = await response.blob()
      if (blob.size === 0) {
        await cache.delete(key)
        return undefined
      }
      this.memory.set(identity, blob)
      return blob
    } catch {
      return undefined
    }
  }

  async put(
    hash: string,
    cacheVersion: number,
    blob: Blob,
    tier: ThumbnailTier = 'compressed'
  ): Promise<void> {
    const identity = thumbnailIdentity(hash, cacheVersion, tier)
    const prefix = `${tier}:${hash}:v`
    const memoryVersions = [...this.memory.keys()]
      .filter((key) => key.startsWith(prefix))
      .map((key) => Number(key.slice(prefix.length)))
      .filter(Number.isSafeInteger)
    if (memoryVersions.some((version) => version > cacheVersion)) return
    for (const key of this.memory.keys()) {
      if (key.startsWith(prefix) && key !== identity) this.memory.delete(key)
    }
    this.memory.set(identity, blob)

    if (this.storage === undefined) return
    this.pendingDiskWrite = this.pendingDiskWrite.then(async () => {
      try {
        const cache = await this.storage?.open(THUMBNAIL_CACHE_NAME)
        if (cache === undefined) return
        const currentKey = thumbnailCacheKey(hash, cacheVersion, tier)
        const keys = await cache.keys()
        const versionedKeys = keys
          .map((key) => ({ key, version: versionFromCacheUrl(key.url, hash, tier) }))
          .filter(
            (entry): entry is { key: Request; version: number } => entry.version !== undefined
          )
        if (versionedKeys.some((entry) => entry.version > cacheVersion)) return
        await Promise.all(
          versionedKeys
            .filter((entry) => entry.key.url !== currentKey)
            .map((entry) => cache.delete(entry.key))
        )
        await cache.put(currentKey, new Response(blob))
      } catch {
        // Cache API is an optional optimization.
      }
    })
    await this.pendingDiskWrite
  }

  async purge(
    hash: string,
    cacheVersion: number,
    tier: ThumbnailTier = 'compressed'
  ): Promise<void> {
    this.memory.delete(thumbnailIdentity(hash, cacheVersion, tier))
    if (this.storage === undefined) return
    try {
      const cache = await this.storage.open(THUMBNAIL_CACHE_NAME)
      await cache.delete(thumbnailCacheKey(hash, cacheVersion, tier))
    } catch {
      // Cache API is an optional optimization.
    }
  }
}
