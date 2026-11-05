<script lang="ts" setup>
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useCollectionStore } from '@/store/collectionStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useConfigStore } from '@/store/configStore'

import ItemSetAsCover from '@Menu/MenuItem/ItemSetAsCover.vue'
import ItemArchive from '@Menu/MenuItem/ItemArchive.vue'
import ItemFavorite from '@Menu/MenuItem/ItemFavorite.vue'
import ItemBatchEditTags from '@Menu/MenuItem/ItemBatchEditTags.vue'
import ItemBatchEditAlbums from '@Menu/MenuItem/ItemBatchEditAlbums.vue'
import ItemDownload from '@Menu/MenuItem/ItemDownload.vue'
import ItemDelete from '@Menu/MenuItem/ItemDelete.vue'
import ItemPermanentlyDelete from '@Menu/MenuItem/ItemPermanentlyDelete.vue'
import ItemRegenerateMetadata from '@Menu/MenuItem/ItemRegenerateMetadata.vue'
import ItemRestore from '@Menu/MenuItem/ItemRestore.vue'
import ItemPhotoAudience from '@Menu/MenuItem/ItemPhotoAudience.vue'

import { getIsolationIdByRoute } from '@utils/getter'
import { shouldShowBatchEditAlbums } from './batchMenuVisibility'

const route = useRoute()
const isolationId = getIsolationIdByRoute(route)
const collectionStore = useCollectionStore(isolationId)
const configStore = useConfigStore('mainId')

const selection = computed(() => collectionStore.descriptor())

const shouldShowSetAsCover = computed(
  () => route.meta.level === 3 && collectionStore.selectedCount(prefetchStore.dataLength) === 1
)

const prefetchStore = usePrefetchStore(isolationId)

const isInTrashedPath = computed(() => route.meta.baseName === 'trashed')

const isInAlbumsPage = computed(() => route.meta.baseName === 'albums')

const canBatchEditAlbums = computed(() => shouldShowBatchEditAlbums(route.meta))
</script>

<template>
  <v-menu>
    <template #activator="{ props: MenuBtn }">
      <v-btn v-bind="MenuBtn" icon="mdi-dots-vertical" aria-label="Batch actions"></v-btn>
    </template>
    <v-list>
      <!-- Conditional Set as Cover -->
      <ItemSetAsCover v-if="shouldShowSetAsCover" />

      <v-divider v-if="shouldShowSetAsCover"></v-divider>

      <ItemPhotoAudience
        v-if="configStore.isAdmin && canBatchEditAlbums"
        :target="selection"
        :isolation-id="isolationId"
      />

      <!-- Archive and Favorite Actions -->
      <ItemArchive :index-list="selection" />
      <ItemFavorite :index-list="selection" />
      <ItemBatchEditTags />
      <ItemBatchEditAlbums v-if="canBatchEditAlbums" />

      <v-divider></v-divider>

      <!-- Download Action -->
      <ItemDownload :index-list="selection" />

      <v-divider></v-divider>

      <!-- Delete or Permanently Delete Actions -->
      <ItemDelete :index-list="selection" v-if="!isInTrashedPath" />
      <ItemRestore :index-list="selection" v-if="isInTrashedPath" />
      <ItemPermanentlyDelete :index-list="selection" v-if="isInTrashedPath" />

      <v-divider></v-divider>

      <!-- Regenerate Action -->
      <ItemRegenerateMetadata v-if="!isInAlbumsPage" :index-list="selection" />
    </v-list>
  </v-menu>
</template>
