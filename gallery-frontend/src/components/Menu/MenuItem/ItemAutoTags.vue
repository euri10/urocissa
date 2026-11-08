<script setup lang="ts">
import { computed, shallowRef } from 'vue'
import { useRoute } from 'vue-router'
import { editAutoTags } from '@/api/autoTags'
import { useConfigStore } from '@/store/configStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { normalizeSelection, selectionCount, type SelectionInput } from '@/type/selection'
import type { IsolationId } from '@/type/types'

const props = defineProps<{
  target: string | SelectionInput
  isolationId: IsolationId
  enabled?: boolean
}>()
const config = useConfigStore('mainId')
const route = useRoute()
const busy = shallowRef(false)
const disabled = computed(() => {
  if (busy.value || !config.canWrite) return true
  if (typeof props.target === 'string') return false
  const prefetch = usePrefetchStore(props.isolationId)
  return prefetch.timestamp === null || selectionCount(normalizeSelection(props.target), prefetch.dataLength) === 0
})
async function change(enabled: boolean): Promise<void> {
  if (disabled.value) return
  busy.value = true
  try {
    await editAutoTags(props.target, enabled, props.isolationId)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <template v-if="config.isAdmin && config.config?.embeddingEnabled && route.meta.baseName !== 'share'">
    <v-list-item
      v-if="enabled !== true"
      prepend-icon="mdi-auto-fix"
      title="Turn Auto tags on"
      :subtitle="typeof target === 'string' ? undefined : 'Photos only'"
      :disabled="disabled"
      @click="change(true)"
    />
    <v-list-item
      v-if="enabled !== false"
      prepend-icon="mdi-auto-fix"
      title="Turn Auto tags off"
      :subtitle="typeof target === 'string' ? undefined : 'Photos only'"
      :disabled="disabled"
      @click="change(false)"
    />
  </template>
</template>
