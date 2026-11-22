<script setup lang="ts">
import { onUnmounted, ref, watch } from 'vue'
import { decideConceptCandidate, requestConceptCandidates, type CandidateAction } from '@/api/autoTags'
import type { GalleryImage, IsolationId } from '@/type/types'
import { useConfigStore } from '@/store/configStore'

const props = defineProps<{ photo: GalleryImage, isolationId: IsolationId }>()
const config = useConfigStore('mainId')
type Decision = 'pending' | 'confirmed' | 'rejected' | 'skipped'
interface Candidate { label: string, decision: Decision }
const candidates = ref<Candidate[]>([])
const loaded = ref(false)
const busy = ref(false)
const error = ref('')
const uncertain = ref(false)
const custom = ref('')
let controller: AbortController | undefined

function reset() {
  controller?.abort()
  candidates.value = []
  loaded.value = false
  custom.value = ''
  error.value = ''
  uncertain.value = false
}
watch(() => props.photo.id, reset)
onUnmounted(reset)

async function generate() {
  if (busy.value || uncertain.value || !config.canWrite || props.photo.autoTagsEnabled === false) return
  const id = props.photo.id
  const request = new AbortController()
  controller = request
  busy.value = true
  error.value = ''
  try {
    const labels = await requestConceptCandidates(id, props.isolationId, request.signal)
    if (!request.signal.aborted && props.photo.id === id) {
      candidates.value = labels.map(label => ({ label, decision: 'pending' }))
      loaded.value = true
    }
  } catch {
    if (!request.signal.aborted) error.value = 'Local suggestions are unavailable. Check the local worker and try again.'
  } finally {
    busy.value = false
  }
}

async function decide(candidate: Candidate, action: CandidateAction) {
  if (busy.value || uncertain.value || !config.canWrite) return
  const id = props.photo.id
  busy.value = true
  error.value = ''
  try {
    if (await decideConceptCandidate(id, candidate.label, action, props.isolationId) && props.photo.id === id) {
      candidate.decision = action === 'confirm' ? 'confirmed' : action === 'reject' ? 'rejected' : 'pending'
    }
  } catch {
    if (props.photo.id === id) {
      uncertain.value = true
      error.value = 'The decision could not be confirmed. It may have been saved or may conflict with an earlier choice; refresh the page before retrying.'
    }
  } finally {
    busy.value = false
  }
}

async function addCustom() {
  const label = custom.value.trim()
  if (label === '') { error.value = 'Enter a tag first.'; return }
  if (candidates.value.some(candidate => candidate.label.toLocaleLowerCase() === label.toLocaleLowerCase())) {
    error.value = 'That tag is already listed.'
    return
  }
  const candidate: Candidate = { label, decision: 'pending' }
  await decide(candidate, 'confirm')
  if (candidate.decision === 'confirmed') {
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
        <v-text-field v-model="custom" label="Add your own tag" maxlength="80" hide-details density="compact" :disabled="busy || uncertain || !config.canWrite" @keyup.enter="addCustom" />
        <v-btn :disabled="busy || uncertain || !config.canWrite" @click="addCustom">Add tag</v-btn>
      </v-sheet>
      <v-card-subtitle class="pa-0 mt-2 text-wrap">A new concept needs multiple examples before it can suggest similar photos.</v-card-subtitle>
    </v-sheet>
  </v-list-item>
</template>
