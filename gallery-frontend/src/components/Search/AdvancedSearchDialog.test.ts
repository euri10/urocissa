import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, h, ref, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import type { AppConfig } from '@/api/config'
import { useConfigStore } from '@/store/configStore'
import { useShareStore } from '@/store/shareStore'
import AdvancedSearchDialog from './AdvancedSearchDialog.vue'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))
vi.mock('vuetify', () => ({ useDisplay: () => ({ smAndDown: ref(false) }) }))

const Stub: FunctionalComponent = (_props, { attrs, slots }) =>
  h('div', attrs, slots.default?.())

async function renderSearch(isAdmin: boolean | undefined, share: boolean, readOnlyMode = false, embeddingEnabled = false) {
  const pinia = createPinia()
  setActivePinia(pinia)
  if (isAdmin !== undefined) {
    useConfigStore('mainId').config = { isAdmin, readOnlyMode, embeddingEnabled } as AppConfig
  }
  if (share) useShareStore('mainId').shareId = 'fixture-share'
  const app = createSSRApp(AdvancedSearchDialog, { modelValue: true, sortOrder: 'descending' })
  app.use(pinia)
  for (const name of [
    'VDialog', 'VCard', 'VCardTitle', 'VIcon', 'VSpacer', 'VBtn', 'VCardText',
    'VTextField', 'VCombobox', 'VLabel', 'VRow', 'VCol', 'VBtnToggle', 'VCardActions'
  ]) app.component(name, Stub)
  return renderToString(app)
}

describe('advanced search audience control', () => {
  it('offers semantic search only to an owner with indexing enabled', async () => {
    expect(await renderSearch(true, false, false, true)).toContain('Describe a photo')
    expect(await renderSearch(true, false, true, true)).toContain('Describe a photo')
    expect(await renderSearch(false, false, false, true)).not.toContain('Describe a photo')
    expect(await renderSearch(true, true, false, true)).not.toContain('Describe a photo')
    expect(await renderSearch(true, false)).not.toContain('Describe a photo')
  })

  it.each([false, true])('offers owner visibility filtering even in read-only mode=%s', async (readOnlyMode) => {
    const html = await renderSearch(true, false, readOnlyMode)
    expect(html).toContain('aria-label="Visibility"')
    expect(html).toContain('value="private"')
    expect(html).toContain('value="public"')
  })

  it.each([false, undefined])('hides visibility controls without owner access: %s', async (isAdmin) => {
    expect(await renderSearch(isAdmin, false)).not.toContain('advanced-audience-toggle')
  })

  it('hides visibility controls inside a share even for the owner', async () => {
    expect(await renderSearch(true, true)).not.toContain('advanced-audience-toggle')
  })
})
