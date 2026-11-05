<script setup lang="ts">
import { computed, ref } from 'vue'
import { setAlbumAudience, type AlbumAudienceAction } from '@/api/albumAudience'
import { useConfigStore } from '@/store/configStore'
import { useMessageStore } from '@/store/messageStore'
import { errorDisplay } from '@/script/utils/errorDisplay'
import type { GalleryAlbum } from '@/type/types'

const props = defineProps<{ album: GalleryAlbum; action: AlbumAudienceAction }>()
const emit = defineEmits<{ close: [] }>()
const configStore = useConfigStore('mainId')
const pending = ref(false)
const error = ref<string | null>(null)
const copy = computed(() => {
  if (props.action === 'publishAll') return {
    title: 'Publish album and all its photos?',
    description: 'This publishes the album and every non-trashed photo and video currently in it. They will be visible to anonymous visitors, including outside this album. Adding photos later does not publish them; they keep their current audience.',
    button: 'Publish all'
  }
  if (props.action === 'private') return {
    title: 'Make album private?',
    description: 'Anonymous visitors will no longer see this album. Its photos keep their current audience; public photos remain visible elsewhere.',
    button: 'Make private'
  }
  return {
    title: 'Publish album?',
    description: 'Anonymous visitors will see this album with only its public photos and videos. Private photos stay private. If it has no public photos, the public album will be empty.',
    button: 'Publish album'
  }
})

async function submit() {
  if (pending.value || !configStore.canWrite || props.album.isTrashed) return
  pending.value = true
  error.value = null
  try {
    await setAlbumAudience(props.album.id, props.action)
    useMessageStore('mainId').success(
      props.action === 'private' ? 'Album is now private.' : 'Album published.'
    )
    emit('close')
  } catch (cause) {
    error.value = errorDisplay(cause)
  } finally {
    pending.value = false
  }
}
</script>

<template>
  <v-dialog
    :model-value="true"
    max-width="480"
    persistent
    scrollable
    aria-labelledby="album-audience-title"
  >
    <v-card border flat rounded="lg" color="surface">
      <v-card-item>
        <v-card-title id="album-audience-title" class="pa-0 text-wrap">
          {{ copy.title }}
        </v-card-title>
        <template #append>
          <v-btn
            icon="mdi-close"
            aria-label="Close album audience dialog"
            variant="text"
            density="comfortable"
            :disabled="pending"
            @click="emit('close')"
          />
        </template>
      </v-card-item>
      <v-progress-linear v-if="pending" indeterminate color="primary" height="2" />
      <v-divider v-else />
      <v-card-text>
        <v-list bg-color="transparent" class="pa-0 mb-4" :lines="false">
          <v-list-item prepend-icon="mdi-image-album" class="px-0">
            <v-list-item-title class="text-wrap text-break">
              {{ album.title?.trim() || 'Untitled' }}
            </v-list-item-title>
            <v-list-item-subtitle>
              {{ album.itemCount }} {{ album.itemCount === 1 ? 'item' : 'items' }}
            </v-list-item-subtitle>
          </v-list-item>
        </v-list>
        <v-alert
          :type="action === 'publishAll' ? 'warning' : 'info'"
          color="on-surface"
          variant="tonal"
        >
          {{ copy.description }}
        </v-alert>
        <v-alert v-if="error !== null" type="error" variant="tonal" class="mt-4 text-break">
          {{ error }}
        </v-alert>
      </v-card-text>
      <v-card-actions>
        <v-spacer />
        <v-btn variant="text" :disabled="pending" @click="emit('close')">Cancel</v-btn>
        <v-btn
          color="primary"
          variant="flat"
          :loading="pending"
          :disabled="!configStore.canWrite || album.isTrashed"
          @click="submit"
        >
          {{ copy.button }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
