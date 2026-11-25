<script setup lang="ts">
import axios from 'axios'
import { onMounted, onUnmounted, shallowRef, watch } from 'vue'
import { z } from 'zod'
import { decideConceptCandidate, type AutoTagAction } from '@/api/autoTags'
import { useConfigStore } from '@/store/configStore'
import type { GalleryImage, IsolationId } from '@/type/types'
import { useMetadataItemLayout } from './useMetadataItemLayout'

const props = defineProps<{ photo: GalleryImage, isolationId: IsolationId }>()
const config = useConfigStore('mainId')
const { metadataChipSize } = useMetadataItemLayout()
const available = shallowRef(false)
const labels = shallowRef<string[]>([])
const pending = shallowRef(false)
const enabled = shallowRef(true)
const busy = shallowRef<string | null>(null)
const error = shallowRef<string | null>(null)
const resultSchema = z.object({ suggestedTags: z.array(z.string()).max(8), pending: z.boolean(), enabled: z.boolean() })
let timer: ReturnType<typeof setTimeout> | undefined
let controller: AbortController | undefined
let disposed = false

function stop(): void {
  clearTimeout(timer)
  controller?.abort()
}

async function load(id: string): Promise<void> {
  stop()
  const request = new AbortController()
  controller = request
  try {
    const response = await axios.get(`/get/suggested-tags/${encodeURIComponent(id)}`, { signal: request.signal, timeout: 5000 })
    const result = resultSchema.parse(response.data)
    if (request.signal.aborted || disposed) return
    available.value = true
    labels.value = result.suggestedTags
    pending.value = result.pending
    enabled.value = result.enabled
    error.value = null
    if (result.pending) timer = setTimeout(() => { void load(id) }, 30000)
  } catch (cause) {
    if (request.signal.aborted || disposed) return
    if (axios.isAxiosError(cause) && cause.response?.status === 404) {
      available.value = false
    } else {
      available.value = true
      error.value = 'Could not load suggested tags.'
    }
  }
}

async function decide(label: string, action: AutoTagAction): Promise<void> {
  if (busy.value !== null || !config.canWrite) return
  const id = props.photo.id
  busy.value = label
  error.value = null
  try {
    if (await decideConceptCandidate(id, label, action, props.isolationId) && !disposed && props.photo.id === id) {
      labels.value = labels.value.filter(candidate => candidate !== label)
      await load(id)
    }
  } catch {
    if (!disposed && props.photo.id === id) error.value = 'Could not save this decision. Please try again.'
  } finally {
    busy.value = null
  }
}

onMounted(() => watch(() => [props.photo.id, props.photo.autoTagsEnabled, props.photo.tags.join('\u0000')], () => {
  available.value = false
  labels.value = []
  void load(props.photo.id)
}, { immediate: true }))
onUnmounted(() => { disposed = true; stop() })
</script>

<template>
  <v-list-item v-if="available" prepend-icon="mdi-lightbulb-outline" title="Suggested tags" lines="two">
    <v-list-item-subtitle class="text-wrap">Suggestions may be wrong. Accepted tags can be public.</v-list-item-subtitle>
    <v-sheet color="transparent" class="d-flex flex-wrap align-center ga-1 py-1" aria-live="polite">
      <v-chip v-for="label in labels" :key="label" variant="tonal" :size="metadataChipSize">
        {{ label }} · suggested
        <template #append>
          <v-btn
            icon="mdi-check" size="24" variant="text" class="ms-1"
            :aria-label="`Accept ${label} as a User tag`" title="Accept as a User tag"
            :disabled="!config.canWrite || busy !== null" :loading="busy === label"
            @click.stop="decide(label, 'confirm')"
          />
          <v-btn
            icon="mdi-close" size="24" variant="text"
            :aria-label="`${label}: Not in this photo`" title="Not in this photo"
            :disabled="!config.canWrite || busy !== null"
            @click.stop="decide(label, 'reject')"
          />
        </template>
      </v-chip>
      <v-list-item-subtitle v-if="labels.length === 0">
        {{ !enabled ? 'Off for this photo' : pending ? 'Waiting for suggestions' : 'No suggestions' }}
      </v-list-item-subtitle>
    </v-sheet>
    <v-alert v-if="error" type="error" density="compact" :text="error" />
  </v-list-item>
</template>
