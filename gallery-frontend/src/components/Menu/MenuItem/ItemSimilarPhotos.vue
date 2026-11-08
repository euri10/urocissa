<script setup lang="ts">
import { computed } from 'vue'
import type { GalleryImage, GalleryVideo } from '@/type/types'
import { useConfigStore } from '@/store/configStore'
import { useShareStore } from '@/store/shareStore'
import { escapeAndWrap } from '@/script/utils/escape'

const props = defineProps<{ photo: Pick<GalleryImage | GalleryVideo, 'id' | 'type'> }>()
const configStore = useConfigStore('mainId')
const shareStore = useShareStore('mainId')
const visible = computed(() => configStore.isAdmin && shareStore.shareId === null
  && configStore.config?.embeddingEnabled === true && props.photo.type === 'image')
const location = computed(() => `/all?search=${encodeURIComponent(`similar:${escapeAndWrap(props.photo.id)}`)}&sort=relevance`)
</script>

<template>
  <v-list-item v-if="visible" prepend-icon="mdi-image-search-outline" :to="location">
    <v-list-item-title class="wrap">Similar photos</v-list-item-title>
  </v-list-item>
</template>
