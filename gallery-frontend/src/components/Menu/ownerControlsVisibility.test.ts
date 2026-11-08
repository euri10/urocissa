import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, h, ref, type Component, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter, type RouteMeta } from 'vue-router'
import type { AppConfig } from '@/api/config'
import { BackendDataParser } from '@/type/schemas'
import { useConfigStore } from '@/store/configStore'
import { useCurrentFrameStore } from '@/store/currentFrameStore'
import { useInitializedStore } from '@/store/initializedStore'
import SingleMenu from './SingleMenu.vue'
import BatchMenu from './BatchMenu.vue'
import AlbumMenu from './AlbumMenu.vue'
import ViewBar from '@/components/NavBar/ViewBar.vue'
import ItemTag from '@/components/View/Metadata/ItemTag.vue'
import ItemAlbum from '@/components/View/Metadata/ItemAlbum.vue'
import MetadataContent from '@/components/View/Metadata/MetadataContent.vue'
import Drawer from '@/components/Page/PageLayout/Drawer.vue'

// Network error redirects are outside this visibility test; avoid loading the
// browser-only application router through the token store.
vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))

const Stub: FunctionalComponent = (_props, { attrs, slots }) =>
  h('div', attrs, [slots.prepend?.(), slots.default?.(), slots.append?.()])

const MenuStub: FunctionalComponent = (_props, { slots }) =>
  h('div', [slots.activator?.({ props: {} }), slots.default?.()])

const ChipStub: FunctionalComponent = (_props, { attrs, slots }) =>
  h('div', { ...attrs, 'data-link': attrs.link === true || attrs.link === '' }, slots.default?.())

function media(type: 'image' | 'video', isTrashed = false) {
  return {
    ...BackendDataParser.parse({
      type, id: 'dummy-photo', pending: false, width: 640, height: 480,
      ext: type === 'image' ? 'jpg' : 'mp4', size: 1234, isTrashed,
      description: 'Existing description', tags: ['public-tag'], albums: []
    }),
    thumbhashUrl: null,
    timestamp: 1_700_000_000_000
  }
}

async function renderControls(
  component: Component,
  props: Record<string, unknown>,
  owner: boolean | undefined,
  captureFrame = false,
  baseName: RouteMeta['baseName'] = 'all',
  level = 1
): Promise<string> {
  const pinia = createPinia()
  setActivePinia(pinia)
  if (owner !== undefined) {
    useConfigStore('mainId').config = { isAdmin: owner, readOnlyMode: !owner, embeddingEnabled: true } as AppConfig
  }
  useInitializedStore('mainId').initialized = true
  if (captureFrame) useCurrentFrameStore('mainId').video = {} as HTMLVideoElement
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{
      path: '/all',
      component: { render: () => null },
      meta: {
        baseName, level,
        getParentPage: () => ({ name: 'all', params: {}, query: {} }),
        getChildPage: () => ({ name: 'all', params: {}, query: {} })
      }
    }]
  })
  await router.push('/all')
  await router.isReady()
  const app = createSSRApp(component, props)
  app.use(pinia)
  app.use(router)
  app.provide('showDrawer', ref(true))
  for (const name of [
    'VList', 'VListItem', 'VListItemTitle', 'VListItemSubtitle', 'VListSubheader',
    'VAvatar', 'VIcon', 'VDivider', 'VBtn', 'VToolbar', 'VToolbarTitle', 'VSpacer',
    'VCardItem', 'VCardText', 'VTextarea', 'VNavigationDrawer', 'VSheet'
  ]) app.component(name, Stub)
  app.component('VMenu', MenuStub)
  app.component('VTooltip', MenuStub)
  app.component('VChip', ChipStub)
  return renderToString(app)
}

describe('owner-only controls', () => {
  it('shows Auto tag controls only for owner photos, never videos or shares', async () => {
    const props = { isolationId: 'mainId', hash: 'dummy-photo', index: 0, database: media('image') }
    expect(await renderControls(SingleMenu, props, true)).toContain('Turn Auto tags off')
    expect(await renderControls(SingleMenu, props, false)).not.toContain('Turn Auto tags')
    expect(await renderControls(SingleMenu, props, true, false, 'share')).not.toContain('Turn Auto tags')
    expect(await renderControls(SingleMenu, { ...props, database: media('video') }, true)).not.toContain('Turn Auto tags')
  })
  it.each([false, undefined])('hides every single-media action for owner=%s', async (owner) => {
    for (const type of ['image', 'video'] as const) {
      for (const isTrashed of [false, true]) {
        const html = await renderControls(SingleMenu, {
          isolationId: 'mainId', hash: 'dummy-photo', index: 0,
          database: media(type, isTrashed)
        }, owner, true)
        expect(html).toContain('Find In Timeline')
        for (const action of [
          'View Original File', 'Download', 'Edit Tags', 'Edit Albums', 'Delete',
          'Restore', 'Reindex', 'Capture Frame as Thumb', 'Rotate Left', 'Publish', 'Make private'
        ]) expect(html).not.toContain(action)
      }
    }
  })

  it('preserves owner menu actions for media and trash states', async () => {
    const props = { isolationId: 'mainId', hash: 'dummy-photo', index: 0 }
    const image = await renderControls(SingleMenu, { ...props, database: media('image') }, true)
    for (const action of [
      'View Original File', 'Download', 'Find In Timeline', 'Edit Tags', 'Edit Albums',
      'Delete', 'Reindex', 'Rotate Left'
    ]) expect(image).toContain(action)
    expect(image).not.toContain('Restore')
    const video = await renderControls(SingleMenu, {
      ...props, database: media('video', true)
    }, true, true)
    for (const action of ['Restore', 'Permanently Delete', 'Capture Frame as Thumb']) {
      expect(video).toContain(action)
    }
    expect(video).not.toContain('Rotate Left')
  })

  it('offers the matching audience action in the owner single-photo menu', async () => {
    const props = { isolationId: 'mainId', hash: 'dummy-photo', index: 0 }
    const privatePhoto = await renderControls(SingleMenu, {
      ...props, database: { ...media('image'), audience: 'private' }
    }, true)
    expect(privatePhoto).toContain('title="Publish"')
    expect(privatePhoto).not.toContain('title="Make private"')
    const publicPhoto = await renderControls(SingleMenu, {
      ...props, database: { ...media('video'), audience: 'public' }
    }, true)
    expect(publicPhoto).toContain('title="Make private"')
    expect(publicPhoto).not.toContain('title="Publish"')
  })

  it('offers photo batch audience actions only to the owner outside the album list', async () => {
    const owner = await renderControls(BatchMenu, {}, true)
    expect(owner).toContain('title="Publish"')
    expect(owner).toContain('title="Make private"')
    const albumPhotos = await renderControls(BatchMenu, {}, true, false, 'albums', 3)
    expect(albumPhotos).toContain('title="Publish"')
    expect(albumPhotos).toContain('title="Make private"')
    for (const html of [
      await renderControls(BatchMenu, {}, false),
      await renderControls(BatchMenu, {}, undefined),
      await renderControls(BatchMenu, {}, true, false, 'albums')
    ]) {
      expect(html).not.toContain('title="Publish"')
      expect(html).not.toContain('title="Make private"')
    }
  })

  it.each([false, true])('gates photo toolbar and album menu for owner=%s', async (owner) => {
    const toolbar = await renderControls(ViewBar, {
      isolationId: 'mainId', hash: 'dummy-photo', index: 0, abstractData: media('image')
    }, owner)
    expect(toolbar.includes('aria-label="Toggle favorite"')).toBe(owner)
    expect(toolbar.includes('aria-label="Toggle archive"')).toBe(owner)
    const menu = await renderControls(AlbumMenu, {
      isolationId: 'mainId', hash: 'dummy-album', index: 0, album: { isTrashed: false }
    }, owner)
    expect(menu).toContain('Find In Timeline')
    expect(menu.includes('Edit Tags')).toBe(owner)
    expect(menu.includes('Delete')).toBe(owner)
    expect(menu.includes('Publish album')).toBe(owner)
    expect(menu.includes('Private album')).toBe(owner)
  })

  it.each(['private', 'public'] as const)('offers separate owner actions for a %s album', async (audience) => {
    const menu = await renderControls(AlbumMenu, {
      isolationId: 'mainId', hash: 'dummy-album', index: 0,
      album: { isTrashed: false, audience }
    }, true)
    expect(menu).toContain('Publish album and all its photos')
    expect(menu.includes('>Publish album<')).toBe(audience === 'private')
    expect(menu.includes('Make private')).toBe(audience === 'public')
  })

  it('does not offer publishing for trashed albums', async () => {
    const menu = await renderControls(AlbumMenu, {
      isolationId: 'mainId', hash: 'dummy-album', index: 0,
      album: { isTrashed: true, audience: 'private' }
    }, true)
    expect(menu).not.toContain('Publish album')
    expect(menu).toContain('Restore')
  })

  it.each([false, true])('renders status chips and metadata editing for owner=%s', async (owner) => {
    const tags = await renderControls(ItemTag, {
      isolationId: 'mainId', index: 0, tags: ['public-tag'], isFavorite: true, isArchived: true
    }, owner)
    expect(tags).toContain('favorite')
    expect(tags).toContain('archived')
    expect(tags).toContain('public-tag')
    expect(tags.includes('mdi-pencil')).toBe(owner)
    expect(tags.match(/data-link="false"/g)?.length ?? 0).toBe(owner ? 0 : 2)
    const albums = await renderControls(ItemAlbum, {
      isolationId: 'mainId', index: 0, albums: []
    }, owner)
    expect(albums.includes('mdi-pencil')).toBe(owner)
    const metadata = await renderControls(MetadataContent, {
      isolationId: 'mainId', hash: 'dummy-photo', index: 0, abstractData: media('image')
    }, owner)
    expect(metadata).toContain('Existing description')
    expect(metadata.includes('data-testid="edit-description"')).toBe(owner)
    expect(metadata.includes('data-testid="readonly-description"')).toBe(!owner)
    expect(metadata.includes('mdi-calendar')).toBe(owner)
  })

  it('keeps anonymous Settings available', async () => {
    const html = await renderControls(Drawer, {}, false)
    expect(html).toContain('title="Settings"')
    expect(html).not.toContain('title="Config"')
    expect(html).not.toContain('title="Trashed"')
  })
})
