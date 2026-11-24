<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import axios from 'axios'
import PageTemplate from './PageLayout/PageTemplate.vue'
import { useConfigStore } from '@/store/configStore'
import { getThumbnailSrc } from '@/script/utils/thumbnail'
import { photoIds, summary, type Judgment, type ReviewRound } from './Acceptance/review'

const route = useRoute()
const router = useRouter()
const config = useConfigStore('mainId')
const kind = computed(() => route.query.kind === 'garden' ? 'garden' : 'random')
const round = ref<ReviewRound | null>(null)
const index = ref(0)
const after = ref(false)
const busy = ref(false)
const error = ref('')
const image = ref('')
const imageReady = ref(false)
const imageFailed = ref(false)
const choices = ref<(Judgment | null)[]>([])
const feedback = ref('')
const teaching = ref('')
const heldOut = ref('')
const plant = ref('')
const separated = ref(false)
const paused = ref(false)
const reviewedThisSitting = ref(0)
const started = ref(Date.now())
let generation = 0
let controller: AbortController | null = null
const current = computed(() => round.value?.photos[index.value])
const predictions = computed(() => after.value ? current.value?.after ?? [] : current.value?.before ?? [])
const held = computed(() => round.value?.photos.filter(photo => photo.role === 'heldOut') ?? [])
const evaluatedPhotos = computed(() => kind.value === 'garden' && after.value ? held.value : round.value?.photos ?? [])
const stats = computed(() => summary(evaluatedPhotos.value, after.value))
const baseline = computed(() => summary(held.value))
const learned = computed(() => summary(held.value, true))
const canSave = computed(() => !busy.value && imageReady.value && config.canWrite && choices.value.every(value => value !== null))
const date = (value: number) => new Date(value).toLocaleDateString()
const percent = (value: number | null) => value === null ? 'undefined (no judged labels)' : `${(value * 100).toFixed(1)}%`

async function load() {
  const active = ++generation
  controller?.abort()
  controller = new AbortController()
  round.value = null
  busy.value = true
  error.value = ''
  try {
    const { data } = await axios.get<ReviewRound | null>('/get/acceptance-review', {
      params: { kind: kind.value }, signal: controller.signal, timeout: 10000
    })
    if (active !== generation) return
    round.value = data
    feedback.value = data?.feedback ?? ''
    after.value = false
    index.value = Math.max(0, data?.photos.findIndex(photo => !photo.seenBefore) ?? 0)
    started.value = Date.now()
    paused.value = false
    reviewedThisSitting.value = 0
  } catch {
    if (active === generation) error.value = 'Could not load the saved review. Sign in as owner and retry.'
  } finally { if (active === generation) busy.value = false }
}

async function start() {
  busy.value = true
  error.value = ''
  try {
    const { data } = await axios.post<ReviewRound>('/post/acceptance-review', {
      kind: kind.value, teaching: photoIds(teaching.value), heldOut: photoIds(heldOut.value),
      plant: plant.value, separationConfirmed: separated.value
    }, { timeout: 30000 })
    round.value = data
    index.value = Math.max(0, data.photos.findIndex(photo => !photo.seenBefore))
    feedback.value = data.feedback
    started.value = Date.now()
  } catch {
    error.value = 'Could not start. Wait for indexing and learning to finish. Garden needs 5–10 distinct teaching photos and 5–10 held-out photos with no previous feedback for the named plant.'
  } finally { busy.value = false }
}

async function save(uncertain = false) {
  if (!round.value || !current.value || busy.value || !imageReady.value || !config.canWrite) return
  if (!uncertain && !canSave.value) return
  const active = generation
  busy.value = true
  error.value = ''
  try {
    const { data } = await axios.put<ReviewRound>('/put/acceptance-review', {
      kind: kind.value, round: round.value.id, hash: current.value.hash, after: after.value,
      judgments: uncertain ? predictions.value.map(() => 'uncertain') : choices.value,
      feedback: feedback.value,
      effortSeconds: Math.min(86400, round.value.effortSeconds + Math.round((Date.now() - started.value) / 1000))
    }, { timeout: 10000 })
    if (active !== generation) return
    round.value = data
    started.value = Date.now()
    reviewedThisSitting.value++
    if (reviewedThisSitting.value >= 10) paused.value = true
    else move(1)
  } catch {
    if (active === generation) error.value = 'The decision was not confirmed saved. Retry; the same photo stays open.'
  } finally { if (active === generation) busy.value = false }
}

function move(delta: number) {
  if (!round.value) return
  let next = index.value + delta
  if (after.value) {
    while (next >= 0 && next < round.value.photos.length && round.value.photos[next]?.role !== 'heldOut') next += delta
  }
  if (next >= 0 && next < round.value.photos.length) index.value = next
}

async function captureAfter() {
  if (!round.value) return
  busy.value = true
  error.value = ''
  try {
    const { data } = await axios.put<ReviewRound>(`/put/acceptance-review/after/${round.value.id}`, null, { timeout: 30000 })
    round.value = data
    after.value = true
    index.value = data.photos.findIndex(photo => photo.role === 'heldOut')
    paused.value = false
    reviewedThisSitting.value = 0
    started.value = Date.now()
  } catch { error.value = 'Could not freeze the after sample. Review the held-out baseline first and wait for learning to finish.' }
  finally { busy.value = false }
}

watch([current, after], async () => {
  const photo = current.value
  image.value = ''
  imageReady.value = false
  imageFailed.value = false
  choices.value = predictions.value.map(tag => tag.judgment)
  if (!photo) return
  const active = generation
  try {
    const { data } = await axios.get<{ data: { token: string } }>(`/get/resource/${photo.hash}`, { timeout: 10000, signal: controller?.signal })
    if (active === generation && current.value.hash === photo.hash) {
      image.value = `${getThumbnailSrc(photo.hash, photo.version)}?token=${encodeURIComponent(data.data.token)}`
    }
  } catch { if (active === generation) imageFailed.value = true }
})
watch(kind, load, { immediate: true })
onBeforeUnmount(() => { generation++; controller?.abort() })
</script>

<template>
  <PageTemplate preset="card">
    <template #content>
      <v-card v-if="config.isAdmin" flat>
        <v-card-item>
          <template #title><v-card-title class="pa-0 text-wrap">Auto tag acceptance review</v-card-title></template>
        </v-card-item>
        <v-card-subtitle class="px-4 text-wrap">Private evaluation · saved on this host · tags and learning are unchanged by your judgments</v-card-subtitle>
        <v-card-text>
          <v-sheet class="d-flex flex-wrap ga-2 mb-4">
            <v-btn :variant="kind === 'random' ? 'tonal' : 'text'" :disabled="busy" @click="router.push('/acceptance')">100 random photos</v-btn>
            <v-btn :variant="kind === 'garden' ? 'tonal' : 'text'" :disabled="busy" @click="router.push('/acceptance?kind=garden')">Garden held-out review</v-btn>
            <v-btn variant="text" :disabled="busy" @click="load">Resume saved review</v-btn>
          </v-sheet>
          <v-progress-linear v-if="busy" indeterminate aria-label="Saving or loading review" class="mb-3" />
          <v-alert v-if="error" type="error" variant="tonal" class="mb-3" :text="error" />
          <template v-if="!round && !busy">
            <v-alert v-if="kind === 'random'" type="info" variant="tonal" class="mb-4">
              Freeze 100 randomly selected non-trashed photos and their current displayed Auto tags. Review 10 at a time; resume later without reshuffling.
              Yes means the label is correct, No means incorrect, and Uncertain preserves doubt. These judgments never confirm or reject a tag.
            </v-alert>
            <template v-else>
              <v-alert type="info" variant="tonal" class="mb-4">
                Choose 5 teaching and 5 held-out garden photos (up to 10 each). Use different dates, seasons and viewpoints when available; keep duplicate bursts in one split.
                Identify a real plant you know. Do not teach or correct held-out photos. This small sample measures this garden only.
                Copy photo links from the existing gallery; only their photo IDs are retained here.
              </v-alert>
              <v-text-field v-model="plant" label="Plant you can identify yourself (for example rhubarb)" maxlength="100" />
              <v-text-field v-model="teaching" label="5–10 teaching photo links, separated by spaces" />
              <v-text-field v-model="heldOut" label="5–10 held-out photo links, separated by spaces" />
              <v-checkbox v-model="separated" label="I checked plant identity, date/season separation and duplicate bursts; the held-out target has never been taught." />
            </template>
            <v-btn color="primary" :disabled="!config.canWrite || (kind === 'garden' && !separated)" @click="start">Freeze {{ kind === 'random' ? 'random sample' : 'garden baseline' }}</v-btn>
          </template>
          <template v-if="round">
            <v-alert type="info" variant="tonal" class="mb-3">
              {{ stats.seen }}/{{ evaluatedPhotos.length }} photos reviewed · {{ stats.covered }} with suggestions · {{ stats.labels }} labels ·
              {{ stats.yes }} correct / {{ stats.no }} incorrect / {{ stats.uncertain }} uncertain.
              Judged-label precision: {{ percent(stats.precision) }}. Conservative precision (uncertain and unreviewed labels count against it): {{ percent(stats.lowerBound) }}.
              {{ stats.off }} off / {{ stats.pending }} pending. {{ stats.pass && kind === 'random' ? '100-photo ≥85% precision criterion met for this frozen round.' : 'Acceptance remains pending.' }}
            </v-alert>
            <v-card-subtitle class="pa-0 text-wrap mb-3">
              Frozen {{ date(round.capturedAt) }} from {{ round.population }} photos. {{ round.model }} · {{ round.vocabulary }}.
              Review what the image actually shows; do not infer identity. Save and next also records a photo with no suggestions.
            </v-card-subtitle>
            <template v-if="kind === 'garden'">
              <v-alert type="warning" variant="tonal" class="mb-3">
                First judge baseline suggestions on every held-out photo. Then open only teaching photos in the gallery, use Info → Build concepts → Suggest concepts for this photo, and Confirm correct generic labels. Use Add your own tag → Add tag for the same exact plant name on every teaching photo.
                Accepted User tags may be visible on public photos. Return here when learning is up to date. A single example is insufficient for a classifier.
              </v-alert>
              <v-sheet class="d-flex flex-wrap ga-2 mb-3">
                <v-btn variant="outlined" :disabled="busy || baseline.seen !== held.length || !!round.afterCapturedAt" @click="captureAfter">Freeze after learning</v-btn>
                <v-btn v-if="round.afterCapturedAt" variant="text" @click="after = !after; index = round!.photos.findIndex(photo => photo.role === 'heldOut')">{{ after ? 'Show baseline' : 'Review after learning' }}</v-btn>
              </v-sheet>
              <v-card-subtitle class="pa-0 text-wrap mb-3">Held-out baseline: {{ baseline.yes }} correct, {{ baseline.no }} incorrect, {{ baseline.uncertain }} uncertain / {{ baseline.labels }} labels. After: {{ learned.yes }} correct, {{ learned.no }} incorrect, {{ learned.uncertain }} uncertain / {{ learned.labels }} labels. Improvement is not assumed.</v-card-subtitle>
            </template>
            <v-alert v-if="paused" type="success" variant="tonal" class="mb-3">
              Ten photos saved. You can stop here and resume this same sample later.
              <v-btn variant="text" @click="paused = false; reviewedThisSitting = 0; started = Date.now(); move(1)">Continue next ten</v-btn>
            </v-alert>
            <v-row v-if="current && !paused">
              <v-col cols="12" md="7">
                <v-card-subtitle class="pa-0 text-wrap mb-2">Photo {{ index + 1 }} · {{ date(current.timestamp) }} · {{ current.role }} · {{ after ? 'after learning' : 'frozen baseline' }}</v-card-subtitle>
                <v-img v-if="image" :src="image" height="350" alt="Photo being evaluated" @load="imageReady = true" @error="imageFailed = true" />
                <v-alert v-if="imageFailed" type="error" variant="tonal" text="Photo unavailable. Do not guess. Retry the saved review after checking the gallery." />
                <v-btn variant="text" :href="`/all/view/${current.hash}`" target="_blank" rel="noopener">Open in existing viewer</v-btn>
              </v-col>
              <v-col cols="12" md="5">
                <v-list border rounded :lines="false" aria-label="Judge frozen suggestions">
                  <v-list-item v-for="(tag, position) in predictions" :key="tag.concept">
                    <v-list-item-title class="text-wrap mb-2">{{ tag.label }}</v-list-item-title>
                    <v-btn-toggle v-model="choices[position]" divided variant="outlined" density="compact" :disabled="busy" :aria-label="`Is ${tag.label} correct?`">
                      <v-btn value="yes" color="success">Yes</v-btn>
                      <v-btn value="no" color="error">No</v-btn>
                      <v-btn value="uncertain">Uncertain</v-btn>
                    </v-btn-toggle>
                  </v-list-item>
                  <v-list-item v-if="!predictions.length" :title="current.pending ? 'Pending when frozen' : !current.enabled ? 'Off when frozen' : 'No displayed Auto tags'" subtitle="Record this photo without inventing labels." />
                </v-list>
                <v-sheet class="d-flex flex-wrap ga-2 mt-3">
                  <v-btn color="primary" :disabled="!canSave" @click="save()">Save and next</v-btn>
                  <v-btn variant="text" :disabled="busy || !imageReady || !config.canWrite" @click="save(true)">Skip as uncertain</v-btn>
                  <v-btn variant="text" :disabled="busy || index === 0" @click="move(-1)">Previous</v-btn>
                  <v-btn variant="text" :disabled="busy || index >= round.photos.length - 1" @click="move(1)">Next</v-btn>
                </v-sheet>
              </v-col>
            </v-row>
            <v-text-field v-model="feedback" class="mt-4" maxlength="2000" label="UX feedback: effort, discoverability, plant identity or uncertainty (saved with next decision)" />
          </template>
        </v-card-text>
      </v-card>
    </template>
  </PageTemplate>
</template>
