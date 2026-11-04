<script setup lang="ts">
import { GalleryAlbum, IsolationId } from '@type/types'
import FindInTimeline from '@Menu/MenuItem/ItemFindInTimeline.vue'
import EditTags from '@Menu/MenuItem/ItemEditTags.vue'
import Delete from '@Menu/MenuItem/ItemDelete.vue'
import PermanentlyDelete from '@Menu/MenuItem/ItemPermanentlyDelete.vue'
import Restore from '@Menu/MenuItem/ItemRestore.vue'
import { useConfigStore } from '@/store/configStore'

const configStore = useConfigStore('mainId')

const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  album: GalleryAlbum
}>()
</script>

<template>
  <v-menu>
    <template #activator="{ props: MenuBtn }">
      <v-btn v-bind="MenuBtn" icon="mdi-dots-vertical"></v-btn>
    </template>
    <v-list>
      <FindInTimeline :hash="props.hash" />
      <template v-if="configStore.isAdmin">
        <v-divider></v-divider>
        <EditTags />
        <Delete v-if="!album.isTrashed" :index-list="[props.index]" />
        <Restore v-if="album.isTrashed" :index-list="[props.index]" />
        <PermanentlyDelete v-if="album.isTrashed" :index-list="[props.index]" />
      </template>
    </v-list>
  </v-menu>
</template>
