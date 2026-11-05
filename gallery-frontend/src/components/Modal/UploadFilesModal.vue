<script setup lang="ts">
import { ref, watch } from 'vue'
import { useConfigStore } from '@/store/configStore'
import {
  useUploadStore,
  type UploadPresentationOptions,
  type UploadTarget
} from '@/store/uploadStore'
import BaseModal from './BaseModal.vue'

const opened = defineModel<boolean>({ required: true })
const props = defineProps<{
  target?: UploadTarget
  presentation?: UploadPresentationOptions
}>()

const configStore = useConfigStore('mainId')
const uploadStore = useUploadStore('mainId')
const publishImmediately = ref(false)

watch(opened, () => {
  publishImmediately.value = false
})

function chooseFiles(): void {
  if (!configStore.canWrite) return
  uploadStore.openFilePicker({
    ...props.target,
    publishImmediately: publishImmediately.value
  }, props.presentation)
  opened.value = false
}
</script>

<template>
  <BaseModal v-model="opened" title="Upload photos and videos" width="480">
    <v-list v-if="configStore.canWrite" border rounded="lg" bg-color="transparent">
      <v-list-item>
        <v-switch
          v-model="publishImmediately"
          label="Publish immediately"
          color="primary"
          hide-details
          inset
        />
      </v-list-item>
    </v-list>
    <v-alert
      type="info"
      variant="tonal"
      density="compact"
      class="mt-4"
    >
      {{ publishImmediately && configStore.canWrite
        ? 'These uploads will be public and visible to everyone. You can make them private later.'
        : 'Uploads are private by default. You can publish them later from the photo menu or selection menu.' }}
    </v-alert>
    <template #actions>
      <v-spacer />
      <v-btn variant="text" @click="opened = false">Cancel</v-btn>
      <v-btn
        color="primary"
        variant="flat"
        prepend-icon="mdi-plus"
        :disabled="!configStore.canWrite"
        @click="chooseFiles"
      >
        Choose Files
      </v-btn>
    </template>
  </BaseModal>
</template>
