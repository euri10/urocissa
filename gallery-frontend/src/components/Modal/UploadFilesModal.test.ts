import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  createRenderer, createSSRApp, h, nextTick, ref, ssrContextKey,
  type Component, type ComponentPublicInstance, type FunctionalComponent
} from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createPinia, setActivePinia } from 'pinia'
import type { AppConfig } from '@/api/config'
import { useConfigStore } from '@/store/configStore'
import { useUploadStore } from '@/store/uploadStore'
import UploadFilesModal from './UploadFilesModal.vue'

vi.mock('@/route/routes', () => ({ default: { push: vi.fn() } }))

// Node Vitest compiles templates for SSR. Use a minimal host to exercise the
// component's real setup/watch lifecycle, and verify its template separately below.
const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
  createElement: () => ({}),
  createText: () => ({}),
  createComment: () => ({}),
  setText: () => undefined,
  setElementText: () => undefined,
  patchProp: () => undefined,
  parentNode: () => null,
  nextSibling: () => null,
  insert: () => undefined,
  remove: () => undefined
})

interface DialogState {
  publishImmediately: boolean
  chooseFiles: () => void
}

function mount(owner = true) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const configStore = useConfigStore('mainId')
  configStore.config = { isAdmin: owner, readOnlyMode: false } as AppConfig
  const picker = vi.spyOn(useUploadStore('mainId'), 'openFilePicker').mockImplementation(() => undefined)
  const opened = ref(true)
  let state: DialogState | undefined
  const Subject: Component = {
    ...UploadFilesModal,
    render: () => null,
    created(this: ComponentPublicInstance) {
      state = (this.$ as unknown as { setupState: DialogState }).setupState
    }
  }
  const app = renderer.createApp({
    setup: () => () => h(Subject, {
      modelValue: opened.value,
      'onUpdate:modelValue': (value: boolean) => { opened.value = value },
      target: { tags: ['selected-tag'] },
      presentation: { showSummaryPanel: false }
    })
  })
  app.use(pinia)
  app.provide(ssrContextKey, { modules: new Set<string>() })
  app.mount({})
  if (state === undefined) throw new Error('Upload dialog did not mount')
  return { app, opened, picker, state, configStore }
}

const Stub: FunctionalComponent = (_props, { attrs, slots }) =>
  h('div', attrs, [slots.default?.(), slots.append?.(), slots.actions?.()])

async function renderModal(owner: boolean): Promise<string> {
  const pinia = createPinia()
  setActivePinia(pinia)
  useConfigStore('mainId').config = { isAdmin: owner, readOnlyMode: false } as AppConfig
  const app = createSSRApp(UploadFilesModal, { modelValue: true })
  app.use(pinia)
  for (const name of [
    'VDialog', 'VCard', 'VToolbar', 'VToolbarTitle', 'VProgressLinear', 'VDivider',
    'VCardText', 'VCardActions', 'VList', 'VListItem', 'VAlert', 'VSpacer', 'VBtn', 'VSwitch'
  ]) app.component(name, Stub)
  return renderToString(app)
}

afterEach(() => { vi.restoreAllMocks() })

describe('upload publication dialog', () => {
  it('starts off and resets after cancel/reopen and after choosing public files', async () => {
    const modal = mount()
    expect(modal.state.publishImmediately).toBe(false)
    modal.state.publishImmediately = true
    modal.opened.value = false
    await nextTick()
    expect(modal.picker).not.toHaveBeenCalled()
    modal.opened.value = true
    await nextTick()
    expect(modal.state.publishImmediately).toBe(false)

    modal.state.publishImmediately = true
    modal.state.chooseFiles()
    await nextTick()
    expect(modal.opened.value).toBe(false)
    expect(modal.picker).toHaveBeenLastCalledWith(
      { tags: ['selected-tag'], publishImmediately: true }, { showSummaryPanel: false }
    )
    modal.opened.value = true
    await nextTick()
    expect(modal.state.publishImmediately).toBe(false)
    modal.state.chooseFiles()
    expect(modal.picker).toHaveBeenLastCalledWith(
      { tags: ['selected-tag'], publishImmediately: false }, { showSummaryPanel: false }
    )
    modal.app.unmount()
  })

  it('renders the switch only for owners and refuses the picker for anonymous users', async () => {
    expect(await renderModal(true)).toContain('label="Publish immediately"')
    expect(await renderModal(false)).not.toContain('label="Publish immediately"')
    const modal = mount(false)
    modal.state.publishImmediately = true
    modal.state.chooseFiles()
    expect(modal.picker).not.toHaveBeenCalled()
    modal.app.unmount()
  })

  it('refuses the picker if owner permission is lost while the dialog is open', () => {
    const modal = mount()
    modal.state.publishImmediately = true
    modal.configStore.config = { isAdmin: false, readOnlyMode: true } as AppConfig
    modal.state.chooseFiles()
    expect(modal.picker).not.toHaveBeenCalled()
    modal.app.unmount()
  })
})
