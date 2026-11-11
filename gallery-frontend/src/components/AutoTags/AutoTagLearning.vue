<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, shallowRef } from 'vue'
import { getAutoTagStatus, recalculateAutoTags, type AutoTagRecalculation } from '@/api/autoTags'
import { useConfigStore } from '@/store/configStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { useSearchFacetStore } from '@/store/searchFacetStore'

const config = useConfigStore('mainId')
const status = shallowRef<AutoTagRecalculation | null>(null)
const busy = shallowRef(false)
const fetchError = shallowRef('')
const requestError = shallowRef('')
const controller = new AbortController()
const active = () => !controller.signal.aborted
let timer: ReturnType<typeof setTimeout> | undefined
let refreshGeneration = 0

const heading = computed(() => {
  if (busy.value || status.value?.running === true) return 'Recalculating Auto tags…'
  if (fetchError.value) return 'Auto tag status unavailable'
  if ((status.value?.lastError ?? '') !== '') return 'Auto tags could not be updated'
  if (requestError.value) return 'Recalculation could not be confirmed'
  if (status.value?.upToDate === true) return 'Auto tags up to date'
  return status.value?.lastCompletedAt == null ? 'Recalculation status unknown' : 'Auto tags awaiting update'
})
const completed = computed(() => status.value?.lastCompletedAt == null
  ? 'Last recalculation time unavailable.'
  : `Last recalculated ${new Date(status.value.lastCompletedAt).toLocaleString()}`)
const result = computed(() => {
  if (status.value?.lastCompletedAt == null) return ''
  const { added, removed } = status.value.changes
  return added + removed === 0 ? 'No suggestions changed.'
    : `${added.toLocaleString()} suggestions added · ${removed.toLocaleString()} removed.`
})

async function updateCollections(): Promise<void> {
  const reload = useCollectionReloadStore('mainId')
  reload.requestMainCollectionReload()
  reload.requestSubCollectionReload()
  const facets = useSearchFacetStore()
  facets.fetched = false
  await facets.fetchFacets()
}

function accept(next: AutoTagRecalculation, refreshCollections = false): void {
  if (!active()) return
  const changed = status.value !== null && next.lastCompletedAt !== status.value.lastCompletedAt
  status.value = next
  if (changed || refreshCollections) {
    requestError.value = ''
    void updateCollections()
  }
}

async function refresh(): Promise<void> {
  if (!active() || !config.isAdmin) return
  const generation = refreshGeneration
  try {
    const next = await getAutoTagStatus(controller.signal)
    if (generation === refreshGeneration) {
      accept(next)
      fetchError.value = ''
    }
  } catch {
    if (active() && generation === refreshGeneration) fetchError.value = 'Could not refresh Auto tag status. Retrying automatically.'
  }
  if (active()) timer = setTimeout(() => { void refresh() }, 2000)
}

async function recalculate(): Promise<void> {
  if (!config.isAdmin || !config.canWrite || busy.value || status.value?.running === true) return
  busy.value = true
  refreshGeneration += 1
  requestError.value = ''
  try {
    const next = await recalculateAutoTags(controller.signal)
    if (active()) {
      refreshGeneration += 1
      accept(next, true)
      fetchError.value = ''
    }
  } catch {
    if (active()) requestError.value = 'Could not complete recalculation. Check the status and try again.'
  } finally {
    busy.value = false
  }
}

onMounted(() => { void refresh() })
onBeforeUnmount(() => {
  controller.abort()
  clearTimeout(timer)
})
</script>

<template>
  <v-sheet v-if="config.isAdmin" id="auto-tag-learning" color="transparent" class="pa-3" aria-label="Auto tag learning">
    <v-list-item prepend-icon="mdi-auto-fix" class="px-0" aria-live="polite">
      <v-list-item-title class="text-wrap text-break">{{ heading }}</v-list-item-title>
      <v-list-item-subtitle class="text-wrap d-block">{{ completed }}</v-list-item-subtitle>
      <v-list-item-subtitle v-if="result" class="text-wrap d-block">{{ result }}</v-list-item-subtitle>
    </v-list-item>
    <v-progress-linear v-if="busy || status?.running" indeterminate color="primary" aria-label="Recalculating Auto tags" class="mb-2" />
    <v-card-subtitle class="pa-0 mb-2 text-wrap">
      Your tag edits and review choices automatically update suggestions across indexed photos.
      Recalculate to retry; the same examples may give the same results. Your choices are preserved.
    </v-card-subtitle>
    <v-alert v-if="requestError || fetchError || status?.lastError" type="warning" variant="tonal" density="compact" class="mb-2 text-break">
      {{ requestError || fetchError || status?.lastError }}
    </v-alert>
    <v-btn prepend-icon="mdi-refresh" variant="tonal" class="text-none" :loading="busy || status?.running" :disabled="!config.canWrite || busy || status?.running" @click="recalculate">
      Recalculate Auto tags
    </v-btn>
    <v-card-subtitle v-if="!config.canWrite" class="pa-0 pt-2 text-wrap">Read-only mode: recalculation is unavailable.</v-card-subtitle>
  </v-sheet>
</template>
