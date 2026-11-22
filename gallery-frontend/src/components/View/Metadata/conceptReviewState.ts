import { reactive, type InjectionKey } from 'vue'

export type Decision = 'pending' | 'confirmed' | 'rejected' | 'skipped'
export interface Candidate { label: string, decision: Decision }
export interface ConceptReviewState {
  photoId: string
  revision: number
  candidates: Candidate[]
  loaded: boolean
  busy: boolean
  error: string
  uncertain: boolean
  custom: string
}

export const conceptReviewKey: InjectionKey<ConceptReviewState> = Symbol('conceptReview')

export function createConceptReviewState(): ConceptReviewState {
  return reactive({ photoId: '', revision: 0, candidates: [], loaded: false, busy: false, error: '', uncertain: false, custom: '' })
}
