<script setup lang="ts">
import { GalleryAlbum, IsolationId } from '@type/types'
import FindInTimeline from '@Menu/MenuItem/ItemFindInTimeline.vue'
import EditTags from '@Menu/MenuItem/ItemEditTags.vue'
import Delete from '@Menu/MenuItem/ItemDelete.vue'
import PermanentlyDelete from '@Menu/MenuItem/ItemPermanentlyDelete.vue'
import Restore from '@Menu/MenuItem/ItemRestore.vue'
import { useConfigStore } from '@/store/configStore'
import { ref } from 'vue'
import AlbumAudienceModal from '@/components/Modal/AlbumAudienceModal.vue'
import type { AlbumAudienceAction } from '@/api/albumAudience'

const configStore = useConfigStore('mainId')
const audienceAction = ref<AlbumAudienceAction | null>(null)

const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  album: GalleryAlbum
}>()
</script>

<template>
  <v-menu max-width="340">
    <template #activator="{ props: MenuBtn }">
      <v-btn v-bind="MenuBtn" icon="mdi-dots-vertical" aria-label="Album actions"></v-btn>
    </template>
    <v-list>
      <FindInTimeline :hash="props.hash" />
      <template v-if="configStore.isAdmin">
        <v-divider></v-divider>
        <v-list-subheader>
          {{ album.audience === 'public' ? 'Public album' : 'Private album' }}
        </v-list-subheader>
        <template v-if="!album.isTrashed">
          <v-list-item
            v-if="album.audience !== 'public'"
            prepend-icon="mdi-earth"
            :disabled="!configStore.canWrite"
            :aria-disabled="!configStore.canWrite"
            @click="audienceAction = 'publish'"
          >
            <v-list-item-title class="text-wrap">Publish album</v-list-item-title>
          </v-list-item>
          <v-list-item
            prepend-icon="mdi-image-multiple"
            :disabled="!configStore.canWrite"
            :aria-disabled="!configStore.canWrite"
            @click="audienceAction = 'publishAll'"
          >
            <v-list-item-title class="text-wrap">Publish album and all its photos</v-list-item-title>
          </v-list-item>
          <v-list-item
            v-if="album.audience === 'public'"
            prepend-icon="mdi-lock-outline"
            :disabled="!configStore.canWrite"
            :aria-disabled="!configStore.canWrite"
            @click="audienceAction = 'private'"
          >
            <v-list-item-title class="text-wrap">Make private</v-list-item-title>
          </v-list-item>
          <v-divider />
        </template>
        <EditTags />
        <Delete v-if="!album.isTrashed" :index-list="[props.index]" />
        <Restore v-if="album.isTrashed" :index-list="[props.index]" />
        <PermanentlyDelete v-if="album.isTrashed" :index-list="[props.index]" />
      </template>
    </v-list>
  </v-menu>
  <AlbumAudienceModal
    v-if="audienceAction !== null && configStore.canWrite"
    :album="album"
    :action="audienceAction"
    @close="audienceAction = null"
  />
</template>
