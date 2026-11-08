<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from 'vue'
import { fetchAutoTags } from '@/api/autoTags'
import type { GalleryImage } from '@/type/types'
import { useMetadataItemLayout } from './useMetadataItemLayout'

const props = defineProps<{ photo: GalleryImage }>()
const { metadataChipSize } = useMetadataItemLayout()
const displayed = computed(() => props.photo.autoTagsEnabled === false
  ? [] : (props.photo.autoTags ?? []).filter(tag => tag.displayed))
function confidenceLabel(confidence: number): string {
  return confidence > 0 && confidence < 0.01 ? '<1%' : `${Math.round(confidence * 100)}%`
}
let timer: ReturnType<typeof setTimeout> | undefined
let controller: AbortController | undefined

function stop(): void {
  clearTimeout(timer)
  controller?.abort()
}

onMounted(() => watch(() => [props.photo.id, props.photo.autoTagsEnabled, props.photo.autoTagsPending, props.photo.tags.join('\u0000')], () => {
  stop()
  if (props.photo.autoTagsEnabled === false) return
  const id = props.photo.id
  const request = new AbortController()
  controller = request
  let attempts = 0
  async function poll(): Promise<void> {
    let pending = true
    try {
      pending = await fetchAutoTags(id, request.signal)
    } catch {
      // The gallery remains usable while the tagger is unavailable. Retry for
      // two minutes at most; mounting another photo starts a fresh bounded check.
    }
    if (!request.signal.aborted && pending && ++attempts < 120) {
      timer = setTimeout(() => { void poll() }, 1000)
    }
  }
  void poll()
}, { immediate: true }))
onUnmounted(stop)
</script>

<template>
  <v-list-item prepend-icon="mdi-auto-fix" title="Auto tags">
    <v-sheet color="transparent" class="d-flex flex-wrap align-center ga-1 py-1" aria-live="polite">
      <v-tooltip
        v-for="tag in displayed"
        :key="tag.concept"
        :text="`Confidence: ${confidenceLabel(tag.confidence)}`"
        location="top"
        open-on-focus
      >
        <template #activator="{ props: tooltip }">
          <v-chip
            v-bind="tooltip"
            variant="outlined"
            color="primary"
            prepend-icon="mdi-auto-fix"
            :size="metadataChipSize"
            tabindex="0"
          >{{ tag.label }}</v-chip>
        </template>
      </v-tooltip>
      <v-list-item-subtitle v-if="displayed.length === 0">
        {{ photo.autoTagsEnabled === false ? 'Off for this photo'
          : photo.autoTagsPending ? 'Waiting for Auto tags' : 'No Auto tags' }}
      </v-list-item-subtitle>
    </v-sheet>
  </v-list-item>
</template>
