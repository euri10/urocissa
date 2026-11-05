import { afterEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, h, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import type { AppConfig } from '@/api/config'
import { albumInfoSchema } from '@/type/schemas'
import type { EditShareData } from '@/type/types'
import { useAlbumStore } from '@/store/albumStore'
import { useConfigStore } from '@/store/configStore'
import LinksPage from './LinksPage.vue'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))
vi.mock('@/components/Modal/EditShareModal.vue', () => ({ default: { render: () => null } }))
vi.mock('@/components/Modal/ShareDeleteConfirmModal.vue', () => ({ default: { render: () => null } }))
vi.mock('./PageLayout/PageTemplate.vue', () => ({
  default: { setup: (_props: unknown, { slots }: { slots: Record<string, () => unknown> }) => () => slots.content?.() }
}))

const Stub: FunctionalComponent = (_props, { attrs, slots }) => h('span', attrs, slots.default?.())
const Table: FunctionalComponent = (_props, { attrs, slots }) => h('table',
  (attrs.items as EditShareData[]).map((item) => h('tr', { 'data-album': item.albumId }, [
    h('td', slots['item.share.description']?.({ item }))
  ])))

afterEach(() => vi.unstubAllGlobals())

async function renderLinks(owner: boolean | undefined) {
  vi.stubGlobal('window', { location: { origin: 'https://gallery.test' } })
  const pinia = createPinia()
  setActivePinia(pinia)
  if (owner !== undefined) useConfigStore('mainId').config = { isAdmin: owner } as AppConfig
  const albums = useAlbumStore('mainId')
  for (const [albumId, audience, hasPrivatePhotos] of [
    ['public-with-private', 'public', true],
    ['private-with-public', 'private', false],
    ['private-with-private', 'private', true],
    ['public-with-public', 'public', false],
    ['missing-aggregate', 'private', undefined]
  ] as const) {
    const shares = [0, 1].map((exp) => ({
      url: `share-${exp}`, description: `${albumId}-${exp}`, password: null,
      showMetadata: false, showDownload: false, showUpload: false, exp
    }))
    albums.albums.set(albumId, albumInfoSchema.parse({
      albumId, albumName: albumId, audience, hasPrivatePhotos,
      shareList: Object.fromEntries(shares.map((share) => [share.url, share]))
    }))
  }
  albums.fetched = true
  const app = createSSRApp(LinksPage)
  app.use(pinia)
  app.component('VDataTable', Table)
  for (const name of ['VSheet', 'VChip']) app.component(name, Stub)
  app.component('VTooltip', (_props, { slots }) => slots.activator?.({ props: {} }))
  return renderToString(app)
}

describe('share private-photo markers', () => {
  it('marks each share containing private photos independently of album audience and expiration', async () => {
    const html = await renderLinks(true)
    expect(html.match(/data-testid="share-private-photos"/g)).toHaveLength(4)
    for (const row of html.match(/<tr\b.*?<\/tr>/g) ?? []) {
      expect(row.includes('share-private-photos')).toBe(row.includes('with-private'))
    }
    expect(html).toContain('prepend-icon="mdi-lock-outline"')
    expect(html).toContain('Private photos')
  })

  it.each([false, undefined])('hides markers without owner authorization: %s', async (owner) => {
    expect(await renderLinks(owner)).not.toContain('share-private-photos')
  })
})
