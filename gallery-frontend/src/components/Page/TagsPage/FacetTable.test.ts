import { describe, expect, it } from 'vitest'
import { createSSRApp, h, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import FacetTable from './FacetTable.vue'
import { searchFacetsSchema } from '@/type/schemas'

describe('tag facets', () => {
  it('preserves owner split counts and renders ordinary visitor counts', async () => {
    const facets = searchFacetsSchema.parse({ tags: [
      { value: 'cigogne', count: 29, userCount: 8, autoCount: 23 },
      { value: 'holiday', count: 3 }
    ], makes: [], models: [] })
    const app = createSSRApp(FacetTable, { valueLabel: 'Tag', items: facets.tags, emptyText: 'No tags' })
    const stub: FunctionalComponent = (_props, { attrs, slots }) => h('div', attrs, slots.default?.())
    app.component('VBtn', stub)
    app.component('VTable', stub)
    const html = await renderToString(app)
    expect(html).toContain('8 yours + 23 auto')
    expect(html).toContain('29 matching items')
    expect(html).toContain('>3<')
  })
})
