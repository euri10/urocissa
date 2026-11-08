<script setup lang="ts">
import { computed, onBeforeUnmount, shallowRef, watch } from 'vue'
import { useRoute } from 'vue-router'
import Home from '@/components/Home/Home.vue'
import PageTemplate from './PageLayout/PageTemplate.vue'
import ReviewToolbar from './ConceptReview/ReviewToolbar.vue'
import { useConfigStore } from '@/store/configStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useInitializedStore } from '@/store/initializedStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { decideAutoTag, previewAutoTagSelection, type AutoTagAction, type AutoTagSelection } from '@/api/autoTags'
import type { SelectionDescriptor } from '@/type/selection'

const route = useRoute()
const config = useConfigStore('mainId')
const prefetch = usePrefetchStore('mainId')
const collection = useCollectionStore('mainId')
const initialized = useInitializedStore('mainId')
const reload = useCollectionReloadStore('mainId')
const concept = computed(() => typeof route.query.concept === 'string' ? route.query.concept : '')
const selected = computed(() => collection.selectedCount(prefetch.dataLength))
const ready = computed(() => initialized.initialized && prefetch.conceptReview != null && prefetch.searchError === null)
const opened = shallowRef(false)
const busy = shallowRef(false)
const error = shallowRef<string | null>(null)
const preview = shallowRef<AutoTagSelection | null>(null)
const pending = shallowRef<{
  action: AutoTagAction
  selection: SelectionDescriptor
  timestamp: number
  concept: string
  label: string
} | null>(null)
let generation = 0

async function open(action: AutoTagAction): Promise<void> {
  if (busy.value || !config.canWrite || !ready.value || selected.value === 0 || prefetch.timestamp === null || !prefetch.conceptReview) return
  const current = ++generation
  pending.value = { action, selection: collection.descriptor(), timestamp: prefetch.timestamp,
    concept: prefetch.conceptReview.concept, label: prefetch.conceptReview.label }
  preview.value = null
  error.value = null
  opened.value = true
  busy.value = true
  try {
    const result = await previewAutoTagSelection(pending.value.selection, 'mainId')
    if (current === generation) preview.value = result
  } catch {
    if (current === generation) error.value = 'Could not load the selected photos. Close and try again.'
  } finally {
    if (current === generation) busy.value = false
  }
}

async function save(): Promise<void> {
  const decision = pending.value
  if (busy.value || !config.canWrite || !decision || (preview.value?.photos ?? 0) === 0) return
  if (prefetch.timestamp !== decision.timestamp) {
    error.value = 'The review changed. Close and select the photos again.'
    return
  }
  busy.value = true
  error.value = null
  const current = generation
  try {
    const saved = await decideAutoTag(decision.selection, decision.concept, decision.action, 'mainId')
    if (saved && current === generation) opened.value = false
  } catch {
    if (current === generation) error.value = 'Could not save these decisions. Please try again.'
  } finally {
    if (current === generation) busy.value = false
  }
}

function refresh(): void { reload.requestMainCollectionReload() }
watch(concept, () => {
  generation += 1
  opened.value = false
  busy.value = false
  pending.value = null
})
onBeforeUnmount(() => { generation += 1 })
</script>

<template>
  <PageTemplate>
    <template #content>
      <Home v-if="config.isAdmin" isolation-id="mainId" :basic-string="null" :search-string="undefined" :review-concept="concept">
        <template #home-toolbar>
          <ReviewToolbar
            :summary="prefetch.conceptReview" :total="prefetch.dataLength"
            :selected="selected" :selecting="collection.editModeOn" :ready="ready" :failed="prefetch.searchError !== null" :can-write="config.canWrite"
            @select="collection.enterEdit()" @all="collection.selectAll()" @cancel="collection.leaveEdit()"
            @confirm="open('confirm')" @reject="open('reject')" @refresh="refresh"
          />
        </template>
      </Home>
    </template>
  </PageTemplate>
  <v-dialog v-model="opened" max-width="480" :persistent="busy">
    <v-card>
      <v-card-item>
        <template #title>
          <v-card-title class="pa-0 text-wrap text-break">{{ pending?.action === 'confirm' ? 'Confirm' : 'Reject' }} {{ pending?.label ?? 'Concept' }}</v-card-title>
        </template>
      </v-card-item>
      <v-progress-linear v-if="busy" indeterminate aria-label="Saving review decisions" />
      <v-card-text>
        <v-alert v-if="error" type="error" variant="tonal" :text="error" class="mb-3" />
        <template v-if="preview && pending">
          <v-alert v-if="pending.action === 'confirm'" type="info" variant="tonal" class="text-break">
            Add {{ pending.label }} as your User tag to {{ preview.photos }} photos.
            It will be visible to visitors on {{ preview.publicPhotos }} published photos.
          </v-alert>
          <v-alert v-else type="warning" variant="tonal" class="text-break">
            Permanently reject {{ pending.label }} on {{ preview.photos }} photos.
            This Concept will never return as an Auto tag on these photos.
          </v-alert>
        </template>
      </v-card-text>
      <v-card-actions class="flex-wrap">
        <v-btn :disabled="busy" @click="opened = false">Cancel</v-btn>
        <v-spacer />
        <v-btn :color="pending?.action === 'reject' ? 'error' : 'primary'" :disabled="busy || !preview?.photos || !config.canWrite" :loading="busy" @click="save">
          {{ pending?.action === 'reject' ? 'Reject permanently' : 'Confirm tags' }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
