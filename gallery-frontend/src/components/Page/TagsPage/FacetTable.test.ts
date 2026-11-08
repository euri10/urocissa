import { describe, expect, it } from 'vitest'
import { createSSRApp, h, type FunctionalComponent } from 'vue'
import { renderToString } from '@vue/server-renderer'
import FacetTable from './FacetTable.vue'
import { searchFacetsSchema } from '@/type/schemas'

describe('tag facets', () => {
  it.each([false, true])('offers Concept review only for reviewable Auto counts, reviewable=%s', async (reviewable) => {
    const reviewed: string[] = []
    const app = createSSRApp(FacetTable, {
      valueLabel: 'Tag', emptyText: 'No tags', reviewable,
      items: [{ value: 'chat', count: 6, autoCount: 2, userCount: 4 }, { value: 'holiday', count: 3 }],
      onReview: (value: string) => reviewed.push(value)
    })
    const stub: FunctionalComponent = (_props, { attrs, slots }) => h('div', attrs, slots.default?.())
    app.component('VTable', stub)
    app.component('VBtn', (_props: unknown, { attrs, slots }) => {
      if (attrs['aria-label'] !== undefined) (attrs.onClick as () => void)()
      return h('button', attrs, slots.default?.())
    })
    const html = await renderToString(app)
    expect(html.includes('Review chat across the library')).toBe(reviewable)
    expect(html).not.toContain('Review holiday across the library')
    expect(reviewed).toEqual(reviewable ? ['chat'] : [])
  })
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
