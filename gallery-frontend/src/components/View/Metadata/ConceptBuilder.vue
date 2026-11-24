<script setup lang="ts">
import { inject, onUnmounted, toRefs, watch } from 'vue'
import { decideConceptCandidate, requestConceptCandidates, type CandidateAction } from '@/api/autoTags'
import type { GalleryImage, IsolationId } from '@/type/types'
import { useConfigStore } from '@/store/configStore'
import { conceptReviewKey, createConceptReviewState, type Candidate } from './conceptReviewState'

const props = defineProps<{ photo: GalleryImage, isolationId: IsolationId }>()
const config = useConfigStore('mainId')
const review = inject(conceptReviewKey, undefined) ?? createConceptReviewState()
const { candidates, loaded, busy, error, uncertain, custom } = toRefs(review)
let controller: AbortController | undefined

function reset(id: string) {
  controller?.abort()
  review.photoId = id
  review.revision += 1
  candidates.value = []
  loaded.value = false
  busy.value = false
  custom.value = ''
  error.value = ''
  uncertain.value = false
}
watch(() => props.photo.id, id => { if (review.photoId !== id) reset(id) }, { immediate: true })
onUnmounted(() => controller?.abort())

async function generate() {
  if (busy.value || uncertain.value || !config.canWrite || props.photo.autoTagsEnabled === false) return
  const id = props.photo.id
  const revision = review.revision
  const request = new AbortController()
  controller = request
  busy.value = true
  error.value = ''
  try {
    const labels = await requestConceptCandidates(id, props.isolationId, request.signal)
    if (!request.signal.aborted && review.photoId === id && review.revision === revision) {
      candidates.value = labels
      loaded.value = true
    }
  } catch {
    if (!request.signal.aborted && review.photoId === id && review.revision === revision) error.value = 'Could not finish local suggestions. The worker may be unavailable or busy, or the request took too long; try again in a moment.'
  } finally {
    if (review.photoId === id && review.revision === revision) busy.value = false
  }
}

async function decide(candidate: Candidate, action: CandidateAction) {
  if (busy.value || uncertain.value || !config.canWrite) return
  const id = props.photo.id
  const revision = review.revision
  busy.value = true
  error.value = ''
  try {
    if (await decideConceptCandidate(id, candidate.label, action, props.isolationId) && review.photoId === id && review.revision === revision) {
      candidate.decision = action === 'confirm' ? 'confirmed' : action === 'reject' ? 'rejected' : 'pending'
    }
  } catch {
    if (review.photoId === id && review.revision === revision) {
      uncertain.value = true
      error.value = 'The decision could not be confirmed. It may have been saved or may conflict with an earlier choice; refresh the page before retrying.'
    }
  } finally {
    if (review.photoId === id && review.revision === revision) busy.value = false
  }
}

async function addCustom(action: 'confirm' | 'reject' = 'confirm') {
  const label = custom.value.trim()
  if (label === '') { error.value = 'Enter a tag first.'; return }
  if (candidates.value.some(candidate => candidate.label.toLocaleLowerCase() === label.toLocaleLowerCase())) {
    error.value = 'That tag is already listed.'
    return
  }
  const candidate: Candidate = { label, decision: 'pending' }
  await decide(candidate, action)
  if (candidate.decision === 'confirmed' || candidate.decision === 'rejected') {
    candidates.value.push(candidate)
    custom.value = ''
    loaded.value = true
  }
}
</script>

<template>
  <v-list-item title="Build concepts" prepend-icon="mdi-tag-plus-outline">
    <v-sheet color="transparent" class="py-2">
      <v-alert type="info" variant="tonal" density="compact" class="mb-2">
        Suggestions use the local AI worker, are limited to a small vocabulary, and may be wrong. Confirmed tags become User tags and may be visible on published photos. Skipping does not save feedback.
      </v-alert>
      <v-btn :disabled="busy || uncertain || !config.canWrite || photo.autoTagsEnabled === false" :loading="busy && !loaded" color="primary" variant="outlined" @click="generate">
        Suggest concepts for this photo
      </v-btn>
      <v-progress-linear v-if="busy" indeterminate color="primary" aria-label="Processing concept request" class="mt-2" />
      <v-alert v-if="photo.autoTagsEnabled === false" type="info" density="compact" variant="tonal" class="mt-2" text="Auto tags are off for this photo." />
      <v-alert v-if="error" type="error" density="compact" variant="tonal" class="mt-2" :text="error" />
      <v-sheet v-if="loaded" color="transparent" class="mt-3" aria-live="polite">
        <v-card-subtitle class="pa-0 mb-2 text-wrap">Review each suggestion; nothing is saved until you choose.</v-card-subtitle>
        <v-alert v-if="candidates.length === 0" type="info" density="compact" variant="tonal" text="No suggestions for this photo. You can add a tag." />
        <v-sheet v-for="candidate in candidates" :key="candidate.label" color="transparent" class="d-flex flex-wrap align-center ga-1 mb-2">
          <v-chip variant="outlined">{{ candidate.label }} · {{ candidate.decision }}</v-chip>
          <template v-if="candidate.decision === 'pending' || candidate.decision === 'skipped'">
            <v-btn size="small" variant="text" color="success" :disabled="busy || uncertain || !config.canWrite" :aria-label="`Confirm ${candidate.label}`" @click="decide(candidate, 'confirm')">Confirm</v-btn>
            <v-btn size="small" variant="text" color="error" :disabled="busy || uncertain || !config.canWrite" :aria-label="`Reject ${candidate.label}`" @click="decide(candidate, 'reject')">Reject</v-btn>
            <v-btn size="small" variant="text" :disabled="busy" :aria-label="`Skip ${candidate.label}`" @click="candidate.decision = 'skipped'">Not sure</v-btn>
          </template>
          <v-btn v-else-if="candidate.decision === 'rejected'" size="small" variant="text" :disabled="busy || uncertain || !config.canWrite" :aria-label="`Undo rejection of ${candidate.label}`" @click="decide(candidate, 'clearRejection')">Undo rejection</v-btn>
        </v-sheet>
        <v-card-subtitle class="pa-0 text-wrap">To correct a confirmed tag, use Edit tags. Undoing a rejection allows this suggestion to appear again.</v-card-subtitle>
      </v-sheet>
      <v-sheet color="transparent" class="d-flex flex-wrap align-center ga-2 mt-3">
        <v-text-field v-model="custom" label="Add your own tag" maxlength="80" hide-details density="compact" :disabled="busy || uncertain || !config.canWrite" @keyup.enter="addCustom()" />
        <v-btn :disabled="busy || uncertain || !config.canWrite" @click="addCustom()">Add tag</v-btn>
        <v-btn color="error" variant="outlined" :disabled="busy || uncertain || !config.canWrite || custom.trim() === ''" @click="addCustom('reject')">Not in this photo</v-btn>
      </v-sheet>
      <v-card-subtitle class="pa-0 mt-2 text-wrap">Enter a concept you know is absent, then choose Not in this photo to teach a negative. No suggestion is required; Undo rejection reverses it.</v-card-subtitle>
      <v-card-subtitle class="pa-0 mt-2 text-wrap">A new concept needs multiple examples before it can suggest similar photos.</v-card-subtitle>
    </v-sheet>
  </v-list-item>
</template>
