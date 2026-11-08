<script setup lang="ts">
import { computed, onMounted, onUnmounted, shallowRef, watch } from 'vue'
import { decideAutoTag, fetchAutoTags, type AutoTagAction } from '@/api/autoTags'
import type { GalleryImage, IsolationId } from '@/type/types'
import { useConfigStore } from '@/store/configStore'
import { useMetadataItemLayout } from './useMetadataItemLayout'

const props = defineProps<{ photo: GalleryImage, isolationId: IsolationId }>()
const config = useConfigStore('mainId')
const busy = shallowRef<string | null>(null)
const error = shallowRef<string | null>(null)
const revision = shallowRef(0)
async function decide(concept: string, action: AutoTagAction): Promise<void> {
  if (busy.value !== null || !config.canWrite) return
  busy.value = concept
  error.value = null
  stop()
  try {
    await decideAutoTag(props.photo.id, concept, action, props.isolationId)
  } catch {
    error.value = 'Could not save this decision. Please try again.'
  } finally {
    busy.value = null
    revision.value += 1
  }
}
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

onMounted(() => watch(() => [props.photo.id, props.photo.autoTagsEnabled, props.photo.autoTagsPending, props.photo.tags.join('\u0000'), revision.value], () => {
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
          >
            {{ tag.label }}
            <template #append>
              <v-btn
                icon="mdi-check" size="24" variant="text" class="ms-1"
                :aria-label="`Confirm ${tag.label} as a User tag`"
                :disabled="!config.canWrite || busy !== null"
                :loading="busy === tag.concept"
                @click.stop="decide(tag.concept, 'confirm')"
              />
              <v-btn
                icon="mdi-close" size="24" variant="text" color="error"
                :aria-label="`Reject ${tag.label} permanently`"
                :disabled="!config.canWrite || busy !== null"
                @click.stop="decide(tag.concept, 'reject')"
              />
            </template>
          </v-chip>
        </template>
      </v-tooltip>
      <v-list-item-subtitle v-if="displayed.length === 0">
        {{ photo.autoTagsEnabled === false ? 'Off for this photo'
          : photo.autoTagsPending ? 'Waiting for Auto tags' : 'No Auto tags' }}
      </v-list-item-subtitle>
    </v-sheet>
    <v-alert v-if="error" type="error" density="compact" :text="error" />
  </v-list-item>
</template>
