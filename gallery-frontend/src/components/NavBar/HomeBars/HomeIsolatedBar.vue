<script setup lang="ts">
import { computed, shallowRef, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useDisplay } from 'vuetify'
import type { GalleryAlbum, GallerySortOrder } from '@type/types'
import type { GallerySearchSubmission } from '@/script/utils/gallerySort'
import { parseGallerySortOrder, hasSemanticSearch } from '@/script/utils/gallerySort'
import { createGallerySearchRouteUpdate } from '@/components/Search/gallerySearchRoute'
import { useCollectionStore } from '@/store/collectionStore'
import { useConfigStore } from '@/store/configStore'
import { useFilterStore } from '@/store/filterStore'
import { useModalStore } from '@/store/modalStore'
import { useCollectionReloadStore } from '@/store/collectionReloadStore'
import { editTitle } from '@utils/createAlbums'
import EditBar from '@/components/NavBar/EditBar.vue'
import HomeTemp from '@/components/Home/HomeTemp.vue'
import CreateShareModal from '@/components/Modal/CreateShareModal.vue'
import LeaveView from '@/components/Menu/MenuButton/BtnLeaveView.vue'
import HomeBarTemplate from '@/components/NavBar/HomeBars/HomeBarTemplate.vue'
import GallerySearchControl from '@/components/Search/GallerySearchControl.vue'

const props = defineProps<{
  album: GalleryAlbum
}>()

const modalStore = useModalStore('mainId')
const collectionStore = useCollectionStore('subId')
const configStore = useConfigStore('mainId')
const filterStore = useFilterStore('subId')
const collectionReloadStore = useCollectionReloadStore('mainId')

const route = useRoute()
const router = useRouter()
const { smAndDown } = useDisplay()

const titleModel = shallowRef('')
const searchQuery = shallowRef<string | null>(null)
const currentSortOrder = computed(() => parseGallerySortOrder(route.query.sort))

async function applySearchState({
  query: rawQuery,
  sortOrder
}: GallerySearchSubmission): Promise<void> {
  const { normalizedSearch, routeQuery } = createGallerySearchRouteUpdate(
    route.query,
    'subSearch',
    { query: rawQuery, sortOrder }
  )
  filterStore.searchString = normalizedSearch === '' ? null : normalizedSearch

  const location = {
    path: route.path,
    query: routeQuery
  }
  if ((sortOrder === 'random' || hasSemanticSearch(rawQuery)) && router.resolve(location).fullPath === route.fullPath) {
    collectionReloadStore.requestSubCollectionReload()
    return
  }

  await router.replace(location)
}

async function handleSearch(query: string): Promise<void> {
  await applySearchState({ query, sortOrder: currentSortOrder.value })
}

async function handleAdvancedSearch(submission: GallerySearchSubmission): Promise<void> {
  await applySearchState(submission)
}

async function handleSort(sortOrder: GallerySortOrder): Promise<void> {
  await applySearchState({
    query: searchQuery.value ?? '',
    sortOrder
  })
}

watch(
  () => props.album.title,
  (title) => {
    titleModel.value = title ?? ''
  },
  { immediate: true }
)

watch(
  () => route.query.subSearch,
  (query) => {
    searchQuery.value = typeof query === 'string' ? query : null
  },
  { immediate: true }
)
</script>

<template>
  <HomeBarTemplate isolation-id="subId">
    <template #content>
      <v-toolbar v-if="!collectionStore.editModeOn" class="position-relative bg-surface">
        <LeaveView />

        <v-card
          variant="flat"
          width="420"
          min-width="0"
          :max-width="smAndDown ? undefined : 'calc(37.5% - 56px)'"
          class="flex-shrink-1"
        >
          <v-card-title>
            <v-text-field
              v-if="configStore.isAdmin"
              v-model="titleModel"
              class="album-title-field"
              data-testid="album-title"
              variant="plain"
              :placeholder="titleModel === '' ? 'Untitled' : undefined"
              @blur="editTitle(props.album, titleModel)"
            />
            <template v-else>{{ props.album.title?.trim() || 'Untitled' }}</template>
          </v-card-title>
        </v-card>

        <GallerySearchControl
          v-model="searchQuery"
          :sort-order="currentSortOrder"
          narrow-activator="icon"
          desktop-layout="center-quarter"
          :show-save-action="false"
          :include-album-media-type="false"
          :desktop-end-gap="0"
          @search="handleSearch"
          @advanced-search="handleAdvancedSearch"
          @sort="handleSort"
        />

        <v-spacer />

        <v-btn
          v-if="configStore.isAdmin"
          icon="mdi-share-variant"
          aria-label="Share album"
          @click="modalStore.showShareModal = true"
        />
        <v-btn
          v-if="configStore.isAdmin"
          icon="mdi-image-plus"
          aria-label="Add album items"
          @click="modalStore.showHomeTempModal = true"
        />
      </v-toolbar>

      <EditBar v-else />

      <HomeTemp v-if="configStore.isAdmin && modalStore.showHomeTempModal" :album="props.album" />
      <CreateShareModal
        v-if="configStore.isAdmin && modalStore.showShareModal"
        :album-id="props.album.id"
        mode="create"
      />
    </template>
  </HomeBarTemplate>
</template>

<style scoped>
.album-title-field :deep(input) {
  font-size: 22px;
  font-weight: 400;
  line-height: 1.175;
  letter-spacing: 0.0073529412em;
  margin-bottom: -8px;
}
</style>
