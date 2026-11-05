<script setup lang="ts">
import { computed, shallowRef } from 'vue'
import { editPhotoAudience, type PhotoAudience } from '@/api/editPhotoAudience'
import { useConfigStore } from '@/store/configStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { normalizeSelection, selectionCount, type SelectionInput } from '@/type/selection'
import type { IsolationId } from '@/type/types'

const props = defineProps<{
  target: string | SelectionInput
  isolationId: IsolationId
  audience?: PhotoAudience
}>()
const configStore = useConfigStore('mainId')
const busy = shallowRef(false)
const disabled = computed(() => {
  if (busy.value || !configStore.canWrite) return true
  if (typeof props.target === 'string') return false
  const prefetch = usePrefetchStore(props.isolationId)
  return prefetch.timestamp === null ||
    selectionCount(normalizeSelection(props.target), prefetch.dataLength) === 0
})

async function changeAudience(audience: PhotoAudience): Promise<void> {
  if (disabled.value) return
  busy.value = true
  try {
    await editPhotoAudience(props.target, audience, props.isolationId)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <template v-if="configStore.isAdmin">
    <v-list-item
      v-if="audience !== 'public'"
      prepend-icon="mdi-earth"
      title="Publish"
      :subtitle="typeof target === 'string' ? undefined : 'Photos and videos only'"
      :disabled="disabled"
      @click="changeAudience('public')"
    />
    <v-list-item
      v-if="audience !== 'private'"
      prepend-icon="mdi-lock-outline"
      title="Make private"
      :subtitle="typeof target === 'string' ? undefined : 'Photos and videos only'"
      :disabled="disabled"
      @click="changeAudience('private')"
    />
  </template>
</template>
