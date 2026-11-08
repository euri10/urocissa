<script setup lang="ts">
import { computed, shallowRef } from 'vue'
import { useRoute } from 'vue-router'
import { decideAutoTag, previewAutoTagSelection, type AutoTagAction, type AutoTagSelection } from '@/api/autoTags'
import { useConfigStore } from '@/store/configStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { normalizeSelection, selectionCount, type SelectionInput } from '@/type/selection'
import type { IsolationId } from '@/type/types'

const props = defineProps<{ target: SelectionInput, isolationId: IsolationId }>()
const config = useConfigStore('mainId')
const route = useRoute()
const opened = shallowRef(false)
const busy = shallowRef(false)
const preview = shallowRef<AutoTagSelection | null>(null)
const concept = shallowRef<string | null>(null)
const error = shallowRef<string | null>(null)
// Freeze the exact selection and snapshot shown in the dialog. The API refuses stale ones.
let selectedTarget: SelectionInput = []
let timestamp: number | null = null
const disabled = computed(() => !config.canWrite || busy.value ||
  usePrefetchStore(props.isolationId).timestamp === null ||
  selectionCount(normalizeSelection(props.target), usePrefetchStore(props.isolationId).dataLength) === 0)

async function open(): Promise<void> {
  if (disabled.value) return
  selectedTarget = structuredClone(normalizeSelection(props.target))
  timestamp = usePrefetchStore(props.isolationId).timestamp
  preview.value = null
  concept.value = null
  error.value = null
  opened.value = true
  busy.value = true
  try {
    preview.value = await previewAutoTagSelection(selectedTarget, props.isolationId)
  } catch {
    error.value = 'Could not load this selection. Close and try again.'
  } finally {
    busy.value = false
  }
}

async function save(action: AutoTagAction): Promise<void> {
  if (busy.value || concept.value === null || preview.value === null || preview.value.photos === 0) return
  if (usePrefetchStore(props.isolationId).timestamp !== timestamp) {
    error.value = 'The collection changed. Close and select the photos again.'
    return
  }
  busy.value = true
  error.value = null
  try {
    if (await decideAutoTag(selectedTarget, concept.value, action, props.isolationId)) opened.value = false
  } catch {
    error.value = 'Could not save these decisions. Please try again.'
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <template v-if="config.isAdmin && config.config?.embeddingEnabled && route.meta.baseName !== 'share'">
    <v-list-item prepend-icon="mdi-check-all" title="Confirm or reject Auto tags" :disabled="disabled" @click.stop="open" />
    <v-dialog v-model="opened" max-width="480" :persistent="busy">
      <v-card>
        <v-card-item title="Confirm or reject Auto tags" />
        <v-progress-linear v-if="busy" indeterminate />
        <v-card-text>
          <v-alert v-if="error" type="error" :text="error" class="mb-4" />
          <template v-if="preview">
            <v-alert v-if="preview.photos === 0" type="info" text="No photos selected. Videos are not auto-tagged." />
            <template v-else>
              <v-list-item :title="`${preview.photos} photos selected`" subtitle="The chosen Concept applies to every selected photo, including photos without a current prediction." class="px-0 text-wrap" />
              <v-select v-model="concept" :items="preview.concepts" item-title="label" item-value="concept" label="Concept" :disabled="busy" class="mt-4" />
              <v-alert type="info" class="mb-3">
                Confirming adds an ordinary User tag to {{ preview.photos }} photos.
                It will be visible to visitors on {{ preview.publicPhotos }} published photos.
              </v-alert>
              <v-alert type="warning" text="Rejection is permanent. This Concept will never return as an Auto tag on these photos." />
            </template>
          </template>
        </v-card-text>
        <v-card-actions class="flex-wrap">
          <v-btn :disabled="busy" @click="opened = false">Cancel</v-btn>
          <v-spacer />
          <v-btn color="error" :disabled="busy || !concept || !preview?.photos" @click="save('reject')">Reject</v-btn>
          <v-btn color="primary" :disabled="busy || !concept || !preview?.photos" @click="save('confirm')">Confirm {{ preview?.photos ?? 0 }}</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </template>
</template>
