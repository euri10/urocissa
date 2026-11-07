<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  getEmbeddingStatus,
  pauseEmbeddingBackfill,
  type EmbeddingStatus
} from '@/api/embeddings'

defineProps<{ canPause: boolean }>()

const status = ref<EmbeddingStatus>()
const refreshing = ref(false)
const saving = ref(false)
const error = ref('')
const controller = new AbortController()
let timer: ReturnType<typeof setTimeout> | undefined
const isActive = () => !controller.signal.aborted

const connection = computed(() => {
  if (!status.value) return 'Connecting to the tagger…'
  if (status.value.online) return `Tagger online · ${status.value.device ?? 'device unknown'}`
  if (status.value.offlineSince !== null) {
    return `Tagger offline since ${new Date(status.value.offlineSince).toLocaleString()}`
  }
  return 'Connecting to the tagger…'
})

const progress = computed(() => {
  const current = status.value
  return current && current.totalPhotos > 0
    ? (current.embeddedPhotos / current.totalPhotos) * 100
    : 0
})

async function refresh() {
  if (!isActive()) return
  if (!saving.value) {
    refreshing.value = true
    try {
      const result = await getEmbeddingStatus(controller.signal)
      if (isActive()) {
        status.value = result
        error.value = ''
      }
    } catch {
      if (isActive()) error.value = 'Unable to refresh photo indexing status. Retrying automatically.'
    } finally {
      refreshing.value = false
    }
  }
  if (isActive()) timer = setTimeout(() => void refresh(), 2000)
}

async function togglePause() {
  if (!status.value || refreshing.value || saving.value) return
  saving.value = true
  try {
    const result = await pauseEmbeddingBackfill(!status.value.backfillPaused, controller.signal)
    if (isActive()) {
      status.value = result
      error.value = ''
    }
  } catch {
    if (isActive()) error.value = 'Could not change background indexing. Please try again.'
  } finally {
    saving.value = false
  }
}

onMounted(() => void refresh())
onBeforeUnmount(() => {
  controller.abort()
  if (timer !== undefined) clearTimeout(timer)
})
</script>

<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">Photo indexing</v-card-title>
      <v-card-text class="text-medium-emphasis">
        Prepares photos for automatic tags and semantic search. Videos are not included.
      </v-card-text>
      <v-progress-linear
        v-if="!status && !error"
        indeterminate
        color="primary"
        aria-label="Loading photo indexing status"
      />
      <v-list bg-color="transparent" aria-live="polite">
        <v-list-item
          :prepend-icon="status?.online ? 'mdi-check-circle-outline' : 'mdi-cloud-off-outline'"
        >
          <v-list-item-title class="text-wrap text-break">{{ connection }}</v-list-item-title>
        </v-list-item>
        <v-list-item v-if="status" class="text-wrap">
          <v-list-item-title class="text-wrap">
            {{ status.embeddedPhotos.toLocaleString() }} of
            {{ status.totalPhotos.toLocaleString() }} photos indexed
          </v-list-item-title>
          <v-list-item-subtitle class="text-wrap d-block">
            {{ status.pendingPhotos.toLocaleString() }} pending
            <template v-if="status.retryingPhotos"> · {{ status.retryingPhotos }} retrying</template>
            <template v-if="status.processing"> · Processing a photo</template>
          </v-list-item-subtitle>
          <v-progress-linear
            v-if="status.totalPhotos > 0"
            :model-value="progress"
            color="primary"
            class="mt-3"
            aria-label="Photo indexing coverage"
          />
        </v-list-item>
        <v-list-item v-if="status">
          <v-list-item-title class="text-wrap">
            {{ status.backfillPaused ? 'Background indexing paused' : 'Background indexing active' }}
          </v-list-item-title>
          <v-list-item-subtitle class="text-wrap d-block">
            New uploads take priority and continue while background indexing is paused.
            A restart resumes background indexing.
          </v-list-item-subtitle>
        </v-list-item>
      </v-list>
      <v-alert v-if="error || status?.lastError" type="warning" variant="tonal" class="mx-4 mb-4 text-break">
        {{ error || status?.lastError }}
      </v-alert>
      <v-card-actions v-if="status" class="px-4 pb-4">
        <v-btn
          variant="tonal"
          class="text-none"
          :prepend-icon="status.backfillPaused ? 'mdi-play' : 'mdi-pause'"
          :disabled="!canPause || refreshing"
          :loading="saving"
          @click="togglePause"
        >
          {{ status.backfillPaused ? 'Resume background indexing' : 'Pause background indexing' }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-col>
</template>
