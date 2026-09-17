import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useConfigStore } from './configStore'
import type { AppConfig } from '@/api/config'

describe('public gallery controls', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('fails closed until owner configuration is loaded', () => {
    const store = useConfigStore('mainId')
    expect(store.isAdmin).toBe(false)
    expect(store.canWrite).toBe(false)
    store.config = { isAdmin: false, readOnlyMode: true } as AppConfig
    expect(store.canWrite).toBe(false)
    store.config = { isAdmin: true, readOnlyMode: false } as AppConfig
    expect(store.canWrite).toBe(true)
    store.config.readOnlyMode = true
    expect(store.canWrite).toBe(false)
  })
})
