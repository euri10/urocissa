import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, ref, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { AppConfig } from '@/api/config'
import { BackendDataParser, rowSchema } from '@/type/schemas'
import { useConfigStore } from '@/store/configStore'
import { useDataStore } from '@/store/dataStore'
import { useImgStore } from '@/store/imgStore'
import { useShareStore } from '@/store/shareStore'
import { scrollActivityKey } from '@/script/hook/useScrollActivity'
import BufferRowBlock from './BufferRowBlock.vue'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))
vi.mock('vuetify/components', async () => {
  const { h } = await import('vue')
  const Stub: FunctionalComponent = (_props, { attrs, slots }) =>
    h('span', attrs, slots.default?.())
  return { VChip: Stub, VIcon: Stub }
})

async function renderBadges(owner: boolean | undefined, mobile: boolean, share = false) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const configStore = useConfigStore('mainId')
  configStore.isMobile = mobile
  if (owner !== undefined) configStore.config = { isAdmin: owner } as AppConfig
  if (share) useShareStore('mainId').shareId = 'fixture-share'

  const records = [
    ...(['image', 'video'] as const).flatMap((type) =>
      (['private', 'public', undefined] as const).map((audience) => ({
        type, audience, id: `${type}-${audience ?? 'missing'}`, pending: false,
        width: 100, height: 80, ext: 'jpg', size: 42
      }))
    ),
    {
      type: 'album', id: 'album', pending: false, title: null, startTime: null,
      endTime: null, lastModifiedTime: 1, cover: null, itemCount: 0, itemSize: 0
    }
  ]
  records.forEach((record, index) => {
    useDataStore('mainId').data.set(index, {
      ...BackendDataParser.parse(record), thumbhashUrl: null, timestamp: 1
    })
    useImgStore('mainId').imgUrl.set(index, `fixture-thumbnail-${index}`)
  })
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/all', component: { render: () => null } }]
  })
  await router.push('/all')
  const app = createSSRApp(BufferRowBlock, {
    isolationId: 'mainId',
    row: rowSchema.parse({
      start: 0, end: records.length - 1, rowIndex: 0,
      displayElements: records.map(() => ({ displayWidth: 100, displayHeight: 80 }))
    })
  })
  app.use(pinia)
  app.use(router)
  app.provide(scrollActivityKey, { active: ref(false), generation: ref(0) })
  return renderToString(app)
}

describe('private photo badges', () => {
  it.each([false, true])('shows private and missing media audiences only for the owner, mobile=%s', async (mobile) => {
    const html = await renderBadges(true, mobile)
    expect(html.match(/data-testid="private-photo-badge"/g)).toHaveLength(4)
    expect(html.match(/aria-label="Private photo"/g)).toHaveLength(4)
    expect(html.match(/icon="mdi-lock-outline"/g)).toHaveLength(4)
    expect(html).toContain('color="surface"')
    // Two public media plus the album have no private badge.
    expect(html.match(/data-testid="gallery-item"/g)).toHaveLength(7)
  })

  it.each([false, undefined])('does not disclose badges with owner=%s', async (owner) => {
    for (const mobile of [false, true]) {
      expect(await renderBadges(owner, mobile)).not.toContain('private-photo-badge')
    }
  })

  it('hides badges in an active share even when the owner is signed in', async () => {
    expect(await renderBadges(true, false, true)).not.toContain('private-photo-badge')
  })
})
