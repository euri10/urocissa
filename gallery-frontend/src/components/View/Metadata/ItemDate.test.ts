import { afterEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, h, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { BackendDataParser } from '@/type/schemas'
import ItemDate from './ItemDate.vue'

const viewer = vi.hoisted(() => ({ owner: undefined as boolean | undefined, share: false }))
vi.mock('@/store/configStore', () => ({
  useConfigStore: () => ({ get isAdmin() { return viewer.owner } })
}))
vi.mock('vue-router', () => ({
  useRoute: () => ({ meta: { baseName: viewer.share ? 'share' : 'home' } })
}))

const VuetifyStub: FunctionalComponent = (_props, { attrs, slots }) =>
  h('div', attrs, [slots.prepend?.(), slots.default?.()])

afterEach(() => vi.unstubAllGlobals())

describe('metadata capture date', () => {
  it.each([
    [true, false, true],
    [false, false, false],
    [undefined, false, false],
    [false, true, false],
    [true, true, false]
  ])('owner=%s share=%s displays date=%s', async (owner, share, visible) => {
    viewer.owner = owner
    viewer.share = share
    vi.stubGlobal('navigator', { language: 'en-US' })
    const database = {
      ...BackendDataParser.parse({
        type: 'image', id: 'image', pending: false, width: 1920, height: 1080,
        size: 1000, ext: 'jpg'
      }),
      timestamp: Date.UTC(2026, 5, 14, 12, 35)
    }
    const app = createSSRApp(ItemDate, { database })
    for (const name of ['VListItem', 'VAvatar', 'VIcon', 'VListItemTitle', 'VListItemSubtitle']) {
      app.component(name, VuetifyStub)
    }
    const html = await renderToString(app)
    expect(html.includes('mdi-calendar')).toBe(visible)
    expect(html.includes('2026')).toBe(visible)
  })
})
