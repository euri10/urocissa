import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, h, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import ItemAutoTags from './ItemAutoTags.vue'
import { BackendDataParser } from '@/type/schemas'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))

async function render(enabled = true, pending = false): Promise<string> {
  const photo = BackendDataParser.parse({
    type: 'image', id: 'photo', pending: false, width: 10, height: 10, size: 1, ext: 'jpg',
    autoTagsEnabled: enabled, autoTagsPending: pending,
    autoTags: [
      { concept: 'cat', label: 'chat', confidence: 0.91, displayed: true },
      { concept: 'animal', label: 'animal', confidence: 0.99, displayed: false }
    ]
  })
  if (photo.type !== 'image') throw new Error('Expected an image')
  const app = createSSRApp(ItemAutoTags, { photo })
  const stub: FunctionalComponent = (_props, { attrs, slots }) => h('div', attrs, slots.default?.())
  for (const name of ['VListItem', 'VSheet', 'VChip', 'VListItemSubtitle']) app.component(name, stub)
  app.component('VTooltip', (_props: unknown, { attrs, slots }) =>
    h('div', attrs, slots.activator?.({ props: {} })))
  return renderToString(app)
}

describe('Auto tag chips', () => {
  it('shows only displayed tags as outlined sparkle chips with confidence', async () => {
    const html = await render()
    expect(html).toContain('chat')
    expect(html).not.toContain('animal')
    expect(html).toContain('variant="outlined"')
    expect(html).toContain('prepend-icon="mdi-auto-fix"')
    expect(html).toContain('Confidence: 91%')
    expect(html).toContain('tabindex="0"')
  })
  it('hides stale predictions immediately when turned off', async () => {
    const html = await render(false)
    expect(html).not.toContain('chat')
    expect(html).toContain('Off for this photo')
  })
})
