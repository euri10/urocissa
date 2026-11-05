<script setup lang="ts">
import { GalleryImage, GalleryVideo, IsolationId } from '@type/types'
import { getSrc } from '@utils/getter'
import ItemViewOriginalFile from '@Menu/MenuItem/ItemViewOriginalFile.vue'
import ItemDownload from '@Menu/MenuItem/ItemDownload.vue'
import ItemFindInTimeline from '@Menu/MenuItem/ItemFindInTimeline.vue'
import ItemEditTags from '@Menu/MenuItem/ItemEditTags.vue'
import ItemEditAlbums from '@Menu/MenuItem/ItemEditAlbums.vue'
import ItemDelete from '@Menu/MenuItem/ItemDelete.vue'
import ItemPermanentlyDelete from '@Menu/MenuItem/ItemPermanentlyDelete.vue'
import ItemRegenerateMetadata from '@Menu/MenuItem/ItemRegenerateMetadata.vue'
import ItemRestore from '@Menu/MenuItem/ItemRestore.vue'
import ItemRegenerateThumbnailByFrame from '@Menu/MenuItem/ItemRegenerateThumbnailByFrame.vue'
import ItemRotateImage from '@Menu/MenuItem/ItemRotateImage.vue'
import { useCurrentFrameStore } from '@/store/currentFrameStore'
import { useConfigStore } from '@/store/configStore'
import ItemPhotoAudience from '@Menu/MenuItem/ItemPhotoAudience.vue'

const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  database: GalleryImage | GalleryVideo
}>()
const currentFrameStore = useCurrentFrameStore(props.isolationId)
const configStore = useConfigStore('mainId')
</script>

<template>
  <v-menu location="start">
    <template #activator="{ props: MenuBtn }">
      <v-btn v-bind="MenuBtn" icon="mdi-dots-vertical" aria-label="Media actions"></v-btn>
    </template>
    <v-list>
      <ItemViewOriginalFile
        v-if="configStore.isAdmin"
        :src="getSrc(database.id, true, database.ext, database.updateAt)"
        :isolation-id="props.isolationId"
        :hash="database.id"
      />
      <ItemDownload v-if="configStore.isAdmin" :index-list="[props.index]" />
      <ItemFindInTimeline :hash="props.hash" />
      <template v-if="configStore.isAdmin">
        <v-divider></v-divider>
        <ItemPhotoAudience
          :target="database.id"
          :isolation-id="props.isolationId"
          :audience="database.audience ?? 'private'"
        />
        <ItemEditTags />
        <ItemEditAlbums />
        <ItemDelete v-if="!database.isTrashed" :index-list="[props.index]" />
        <ItemRestore v-if="database.isTrashed" :index-list="[props.index]" />
        <ItemPermanentlyDelete v-if="database.isTrashed" :index-list="[props.index]" />
        <v-divider></v-divider>
        <ItemRegenerateMetadata :index-list="[props.index]" />
        <ItemRegenerateThumbnailByFrame v-if="currentFrameStore.video !== null" />
        <ItemRotateImage v-if="database.type === 'image'" />
      </template>
    </v-list>
  </v-menu>
</template>
