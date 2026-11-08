<script setup lang="ts">
import type { PrefetchReturn } from '@/type/types'

defineProps<{
  summary: PrefetchReturn['conceptReview'] | null
  total: number
  selected: number
  selecting: boolean
  ready: boolean
  failed: boolean
  canWrite: boolean
}>()
const emit = defineEmits<{
  select: []
  all: []
  cancel: []
  confirm: []
  reject: []
  refresh: []
}>()
</script>

<template>
  <v-sheet color="surface" class="pa-3" aria-label="Concept review controls">
    <v-card-item class="pa-0 pb-2">
      <template #title>
        <v-card-title class="pa-0 text-wrap text-break">{{ summary?.label ?? 'Concept review' }}</v-card-title>
      </template>
      <template #subtitle>
        <v-card-subtitle class="pa-0 text-wrap">
          {{ total }} Auto-tagged photos across the library, including Trash. Least confident first.
        </v-card-subtitle>
      </template>
    </v-card-item>
    <v-sheet v-if="summary" color="transparent" class="d-flex flex-wrap ga-2 mb-2" aria-live="polite">
      <v-chip size="small" variant="outlined">{{ summary.positiveExamples }}/5 positive examples</v-chip>
      <v-chip size="small" variant="outlined">{{ summary.negativeExamples }}/5 negative examples</v-chip>
      <v-card-subtitle class="pa-0 text-wrap w-100">
        {{ summary.positiveExamples >= 5 && summary.negativeExamples >= 5
          ? 'Enough examples for a learned classifier.' : 'The classifier needs at least 5 of each.' }}
        Counts use photos with current embeddings.
      </v-card-subtitle>
    </v-sheet>
    <v-progress-linear v-if="!ready && !failed" indeterminate color="primary" aria-label="Loading Concept review" class="mb-2" />
    <v-sheet color="transparent" class="d-flex flex-wrap align-center ga-2">
      <v-btn v-if="!selecting" prepend-icon="mdi-checkbox-multiple-marked-outline" :disabled="!ready || total === 0 || !canWrite" @click="emit('select')">Select photos</v-btn>
      <template v-else>
        <v-btn variant="text" @click="emit('cancel')">Cancel selection</v-btn>
        <v-btn variant="text" :disabled="!ready || total === 0" @click="emit('all')">Select all</v-btn>
        <v-chip size="small">{{ selected }} selected</v-chip>
        <v-btn prepend-icon="mdi-check" color="primary" :disabled="!ready || selected === 0 || !canWrite" @click="emit('confirm')">Confirm selected</v-btn>
        <v-btn prepend-icon="mdi-close" color="error" variant="outlined" :disabled="!ready || selected === 0 || !canWrite" @click="emit('reject')">Reject selected</v-btn>
      </template>
      <v-btn prepend-icon="mdi-refresh" variant="text" @click="emit('refresh')">Refresh review</v-btn>
    </v-sheet>
    <v-alert v-if="!canWrite" type="info" variant="tonal" density="compact" class="mt-2" text="Read-only mode: you can review photos, but cannot confirm or reject tags." />
  </v-sheet>
</template>
