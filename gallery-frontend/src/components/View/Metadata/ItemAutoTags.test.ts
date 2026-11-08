import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, h, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import ItemAutoTags from './ItemAutoTags.vue'
import { BackendDataParser } from '@/type/schemas'
import { createPinia, setActivePinia } from 'pinia'
import { useConfigStore } from '@/store/configStore'
import { decideAutoTag } from '@/api/autoTags'
import type { AppConfig } from '@/api/config'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))
vi.mock('@/api/autoTags', () => ({ decideAutoTag: vi.fn().mockResolvedValue(true), fetchAutoTags: vi.fn() }))
const actions = new Map<string, { disabled: boolean, to: unknown, click: (event: { stopPropagation: () => void }) => Promise<void> }>()
function action(label: string) {
  const control = actions.get(label)
  if (control === undefined) throw new Error(`Missing control: ${label}`)
  return control
}
beforeEach(() => {
  setActivePinia(createPinia())
  useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: false } as AppConfig
  actions.clear()
  vi.clearAllMocks()
})

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
  const app = createSSRApp(ItemAutoTags, { photo, isolationId: 'detailId' })
  const stub: FunctionalComponent = (_props, { attrs, slots }) => h('div', attrs, slots.default?.())
  for (const name of ['VListItem', 'VSheet', 'VListItemSubtitle']) app.component(name, stub)
  app.component('VChip', (_props: unknown, { attrs, slots }) => h('div', attrs, [slots.default?.(), slots.append?.()]))
  app.component('VBtn', (_props: unknown, { attrs }) => {
    actions.set(String(attrs['aria-label']), { disabled: Boolean(attrs.disabled), to: attrs.to, click: attrs.onClick as (event: { stopPropagation: () => void }) => Promise<void> })
    return h('button', attrs)
  })
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
    expect(action('Review chat across the library').to).toEqual({ name: 'review', query: { concept: 'cat' } })
  })
  it('hides stale predictions immediately when turned off', async () => {
    const html = await render(false)
    expect(html).not.toContain('chat')
    expect(html).toContain('Off for this photo')
  })
  it('confirms or permanently rejects the stable Concept through accessible controls', async () => {
    await render()
    await action('Confirm chat as a User tag').click({ stopPropagation: vi.fn() })
    expect(decideAutoTag).toHaveBeenCalledWith('photo', 'cat', 'confirm', 'detailId')
    await action('Reject chat permanently').click({ stopPropagation: vi.fn() })
    expect(decideAutoTag).toHaveBeenCalledWith('photo', 'cat', 'reject', 'detailId')
  })
  it('disables actions in read-only mode', async () => {
    useConfigStore('mainId').config = { isAdmin: true, readOnlyMode: true } as AppConfig
    await render()
    const control = action('Confirm chat as a User tag')
    expect(control.disabled).toBe(true)
    await control.click({ stopPropagation: vi.fn() })
    expect(decideAutoTag).not.toHaveBeenCalled()
  })
})
