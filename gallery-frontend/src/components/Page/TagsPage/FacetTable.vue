<script setup lang="ts">
import { computed } from 'vue'
import type { FacetValueInfo } from '@type/types'

const props = defineProps<{
  valueLabel: string
  items: readonly FacetValueInfo[]
  emptyText: string
}>()

const emit = defineEmits<{
  select: [value: string]
}>()

const hasSplitCounts = computed(() => props.items.some((item) => item.userCount !== undefined))

function selectValue(value: string): void {
  emit('select', value)
}
</script>

<template>
  <v-table hover class="facet-table">
    <thead>
      <tr>
        <th scope="col">{{ props.valueLabel }}</th>
        <th scope="col" :class="hasSplitCounts ? 'w-50 text-end' : 'facet-count-column text-end'">
          Items
        </th>
      </tr>
    </thead>
    <tbody>
      <tr v-if="props.items.length === 0">
        <td colspan="2" class="text-medium-emphasis">{{ props.emptyText }}</td>
      </tr>
      <tr v-for="item in props.items" :key="item.value">
        <td>
          <v-btn
            slim
            class="text-body-small"
            variant="tonal"
            @click="selectValue(item.value)"
          >
            {{ item.value }}
          </v-btn>
        </td>
        <td class="text-end" :title="`${item.count} matching items`">
          <template v-if="item.userCount !== undefined && item.autoCount !== undefined">
            {{ item.userCount }} yours + {{ item.autoCount }} auto
          </template>
          <template v-else>{{ item.count }}</template>
        </td>
      </tr>
    </tbody>
  </v-table>
</template>

<style scoped>
.facet-table :deep(table) {
  table-layout: fixed;
}

.facet-count-column {
  width: 96px;
}
</style>
