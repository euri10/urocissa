<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { useDisplay } from 'vuetify'
import { useSearchFacetStore } from '@/store/searchFacetStore'
import { useConfigStore } from '@/store/configStore'
import { useShareStore } from '@/store/shareStore'
import {
  buildAdvancedSearchFilter,
  createEmptyAdvancedSearchCriteria,
  type AdvancedSearchCriteria
} from './advancedSearch'
import type { GallerySortOrder } from '@/type/types'
import type { GallerySearchSubmission } from '@/script/utils/gallerySort'

const isOpen = defineModel<boolean>({ required: true })

const props = withDefaults(
  defineProps<{
    sortOrder: GallerySortOrder
    includeAlbumMediaType?: boolean
  }>(),
  {
    includeAlbumMediaType: true
  }
)

const emit = defineEmits<{
  search: [submission: GallerySearchSubmission]
}>()

const { smAndDown } = useDisplay()
const searchFacetStore = useSearchFacetStore()
const configStore = useConfigStore('mainId')
const shareStore = useShareStore('mainId')
const canFilterAudience = computed(() => configStore.isAdmin && shareStore.shareId === null)
const canSearchSemantic = computed(
  () => canFilterAudience.value && configStore.config?.embeddingEnabled === true
)
const criteria = reactive<AdvancedSearchCriteria>(
  createEmptyAdvancedSearchCriteria(props.sortOrder)
)
const hasSemanticText = computed(() => (criteria.semantic?.trim() ?? '') !== '')
const sortButtonWidth = computed(() => smAndDown.value && hasSemanticText.value ? '50%' : undefined)
watch(hasSemanticText, (hasText, previouslyHadText) => {
  if (hasText && !previouslyHadText) criteria.sortOrder = 'relevance'
  if (!hasText && criteria.sortOrder === 'relevance') criteria.sortOrder = 'descending'
})
const tagItems = computed(() => searchFacetStore.tags.map((facet) => facet.value))
const makeItems = computed(() => searchFacetStore.makes.map((facet) => facet.value))
const modelItems = computed(() => searchFacetStore.models.map((facet) => facet.value))

function clearAll(): void {
  Object.assign(criteria, createEmptyAdvancedSearchCriteria())
}

function submitSearch(): void {
  emit('search', {
    query: buildAdvancedSearchFilter({
      ...criteria,
      semantic: canSearchSemantic.value ? (criteria.semantic ?? '') : '',
      audience: canFilterAudience.value ? criteria.audience : 'all'
    }),
    sortOrder: criteria.sortOrder === 'relevance' && (!canSearchSemantic.value || !hasSemanticText.value)
      ? 'descending'
      : criteria.sortOrder
  })
  isOpen.value = false
}
</script>

<template>
  <v-dialog v-model="isOpen" :fullscreen="smAndDown" max-width="600">
    <v-card class="d-flex flex-column">
      <v-card-title class="d-flex align-center flex-shrink-0">
        <v-icon class="mr-2">mdi-tune</v-icon>
        <span class="text-h6">Search Options</span>
        <v-spacer />
        <v-btn icon="mdi-close" variant="text" aria-label="Close" @click="isOpen = false" />
      </v-card-title>

      <v-card-text class="flex-grow-1 overflow-y-auto">
        <v-text-field
          v-if="canSearchSemantic"
          v-model="criteria.semantic"
          label="Describe a photo"
          placeholder="e.g. child on a bike, sunset on the beach"
          hint="Searches indexed photos by description. Videos and pending photos are excluded."
          persistent-hint
          prepend-inner-icon="mdi-image-search-outline"
          variant="outlined"
          density="comfortable"
          clearable
          autocomplete="off"
          class="mb-4"
        />

        <v-text-field
          v-model="criteria.keyword"
          label="Keyword"
          variant="outlined"
          density="comfortable"
          clearable
          hide-details
          autocomplete="off"
          class="mb-4"
        />

        <v-text-field
          v-model="criteria.filename"
          label="Filename"
          variant="outlined"
          density="comfortable"
          clearable
          hide-details
          autocomplete="off"
          class="mb-4"
        />

        <v-combobox
          v-model="criteria.tag"
          :items="tagItems"
          :hide-no-data="false"
          label="Tag"
          menu-icon="mdi-menu-down"
          no-data-text="No tags available"
          variant="outlined"
          density="comfortable"
          clearable
          hide-details
          autocomplete="off"
          class="mb-4"
        />

        <v-text-field
          v-model="criteria.extension"
          label="File Extension"
          placeholder="e.g. jpg, png, mp4"
          variant="outlined"
          density="comfortable"
          clearable
          hide-details
          autocomplete="off"
          class="mb-4"
        />

        <v-label text="Camera" class="text-subtitle-2 mb-2" />
        <v-row density="comfortable" class="mb-4" aria-label="Camera">
          <v-col cols="6">
            <v-combobox
              v-model="criteria.cameraMake"
              :items="makeItems"
              :hide-no-data="false"
              label="Make"
              menu-icon="mdi-menu-down"
              no-data-text="No camera makes available"
              variant="outlined"
              density="comfortable"
              clearable
              hide-details
              autocomplete="off"
            />
          </v-col>
          <v-col cols="6">
            <v-combobox
              v-model="criteria.cameraModel"
              :items="modelItems"
              :hide-no-data="false"
              label="Model"
              menu-icon="mdi-menu-down"
              no-data-text="No camera models available"
              variant="outlined"
              density="comfortable"
              clearable
              hide-details
              autocomplete="off"
            />
          </v-col>
        </v-row>

        <v-label
          for="advanced-media-type-toggle"
          text="Media Type"
          class="text-subtitle-2 mb-2"
        />
        <v-btn-toggle
          id="advanced-media-type-toggle"
          v-model="criteria.mediaType"
          mandatory
          color="primary"
          border
          divided
          class="d-flex"
          aria-label="Media type"
        >
          <v-btn value="all" class="flex-1-1-0">All</v-btn>
          <v-btn value="image" class="flex-1-1-0">Image</v-btn>
          <v-btn value="video" class="flex-1-1-0">Video</v-btn>
          <v-btn v-if="props.includeAlbumMediaType" value="album" class="flex-1-1-0">
            Album
          </v-btn>
        </v-btn-toggle>

        <template v-if="canFilterAudience">
          <v-label
            for="advanced-audience-toggle"
            text="Visibility"
            class="text-subtitle-2 mt-4 mb-2"
          />
          <v-btn-toggle
            id="advanced-audience-toggle"
            v-model="criteria.audience"
            mandatory
            color="primary"
            border
            divided
            class="d-flex"
            aria-label="Visibility"
          >
            <v-btn value="all" class="flex-1-1-0">All</v-btn>
            <v-btn value="private" class="flex-1-1-0">Private</v-btn>
            <v-btn value="public" class="flex-1-1-0">Public</v-btn>
          </v-btn-toggle>
        </template>

        <v-label
          for="advanced-sort-order-toggle"
          text="Sort Order"
          class="text-subtitle-2 mt-4 mb-2"
        />
        <v-btn-toggle
          id="advanced-sort-order-toggle"
          v-model="criteria.sortOrder"
          mandatory
          color="primary"
          border
          divided
          class="d-flex flex-wrap h-auto"
          aria-label="Sort order"
        >
          <v-btn value="descending" height="48" :min-width="sortButtonWidth" class="flex-1-1-0">Descending</v-btn>
          <v-btn value="ascending" height="48" :min-width="sortButtonWidth" class="flex-1-1-0">Ascending</v-btn>
          <v-btn value="random" height="48" :min-width="sortButtonWidth" class="flex-1-1-0">Random</v-btn>
          <v-btn v-if="canSearchSemantic && hasSemanticText" value="relevance" height="48" :min-width="sortButtonWidth" class="flex-1-1-0">Relevance</v-btn>
        </v-btn-toggle>
      </v-card-text>

      <v-card-actions class="flex-shrink-0">
        <v-btn variant="text" @click="clearAll">Clear All</v-btn>
        <v-spacer />
        <v-btn color="primary" variant="elevated" @click="submitSearch">Search</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
