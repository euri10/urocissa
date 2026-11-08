import { describe, expect, it } from 'vitest'
import { createSSRApp, h, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import type { AppConfig } from '@/api/config'
import { useConfigStore } from '@/store/configStore'
import { useShareStore } from '@/store/shareStore'
import ItemSimilarPhotos from './ItemSimilarPhotos.vue'

const id = 'a'.repeat(64)
async function render({ owner = true, share = false, enabled = true, type = 'image', readOnlyMode = false }:
  { owner?: boolean; share?: boolean; enabled?: boolean; type?: 'image' | 'video'; readOnlyMode?: boolean } = {}) {
  const pinia = createPinia()
  setActivePinia(pinia)
  useConfigStore('mainId').config = { isAdmin: owner, embeddingEnabled: enabled, readOnlyMode } as AppConfig
  if (share) useShareStore('mainId').shareId = 'fixture-share'
  const app = createSSRApp(ItemSimilarPhotos, { photo: { id, type } })
  app.use(pinia)
  const Item: FunctionalComponent = (_props, { attrs, slots }) => h('a', { href: attrs.to }, slots.default?.())
  const Title: FunctionalComponent = (_props, { slots }) => h('span', slots.default?.())
  app.component('VListItem', Item)
  app.component('VListItemTitle', Title)
  return renderToString(app)
}

describe('Similar photos menu item', () => {
  it.each([false, true])('opens the ordinary gallery with relevance sort, readOnlyMode=%s', async (readOnlyMode) => {
    const html = await render({ readOnlyMode })
    expect(html).toContain('Similar photos')
    expect(html).toContain(`/all?search=similar%3A%22${id}%22&amp;sort=relevance`)
  })

  it('is hidden for visitors, shares, videos and disabled indexing', async () => {
    for (const state of [{ owner: false }, { share: true }, { type: 'video' as const }, { enabled: false }]) {
      expect(await render(state)).not.toContain('Similar photos')
    }
  })
})
