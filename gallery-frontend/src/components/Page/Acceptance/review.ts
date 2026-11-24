export type Judgment = 'yes' | 'no' | 'uncertain'
export interface Suggestion { concept: string; label: string; judgment: Judgment | null }
export interface ReviewPhoto {
  hash: string; version: number; timestamp: number; role: string; enabled: boolean; pending: boolean
  before: Suggestion[]; after: Suggestion[] | null; seenBefore: boolean; seenAfter: boolean
}
export interface ReviewRound {
  id: string; kind: 'random' | 'garden'; capturedAt: number; model: string; vocabulary: string
  materializedAt: number; libraryRevision: number; population: number; plant: string
  separationConfirmed: boolean; afterCapturedAt: number | null; photos: ReviewPhoto[]
  feedback: string; effortSeconds: number
}

export function summary(photos: ReviewPhoto[], after = false) {
  let yes = 0, no = 0, uncertain = 0, labels = 0, seen = 0, covered = 0, pending = 0, off = 0
  for (const photo of photos) {
    const suggestions = after ? photo.after ?? [] : photo.before
    if (after ? photo.seenAfter : photo.seenBefore) seen++
    if (suggestions.length) covered++
    if (photo.pending) pending++
    if (!photo.enabled) off++
    labels += suggestions.length
    for (const tag of suggestions) {
      if (tag.judgment === 'yes') yes++
      if (tag.judgment === 'no') no++
      if (tag.judgment === 'uncertain') uncertain++
    }
  }
  const precision = yes + no > 0 ? yes / (yes + no) : null
  const lowerBound = labels > 0 ? yes / labels : null
  return { yes, no, uncertain, labels, seen, covered, pending, off, precision, lowerBound,
    pass: photos.length === 100 && seen === 100 && lowerBound !== null && lowerBound >= 0.85 }
}

export function photoIds(text: string): string[] {
  return text.split(/[\s,]+/).filter(Boolean).map(value => {
    if (/^[0-9a-f]{64}$/.test(value)) return value
    try {
      const url = new URL(value)
      if (url.origin !== 'https://photos.sslc.rocks') return ''
      return /(?:^|\/)([0-9a-f]{64})(?:\/|$)/.exec(url.pathname)?.[1] ?? ''
    } catch { return '' }
  }).filter(Boolean)
}
