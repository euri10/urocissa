import { afterEach, describe, expect, it, vi } from 'vitest'
import { generateJsonString } from './generateJson'

describe('audience search expressions', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it.each(['private', 'public'])('parses quoted and bare %s audience', (audience) => {
    for (const expression of [`audience:"${audience}"`, `audience:${audience}`]) {
      expect(JSON.parse(generateJsonString(expression))).toEqual({ Audience: audience })
    }
  })

  it('combines audience with nested Boolean expressions and metadata filters', () => {
    expect(JSON.parse(generateJsonString(
      'and(audience:"private",or(tag:"family",not(audience:public)),type:"image")'
    ))).toEqual({
      And: [
        { Audience: 'private' },
        { Or: [{ Tag: 'family' }, { Not: { Audience: 'public' } }] },
        { ExtType: 'image' }
      ]
    })
  })

  it('leaves audience syntax inside quoted text unchanged', () => {
    const text = 'audience:"private" or audience:public and favorite:true'
    const expression = `any:${JSON.stringify(text)}`
    expect(JSON.parse(generateJsonString(expression))).toEqual({ Any: text })
    expect(JSON.parse(generateJsonString('tag:"private"'))).toEqual({ Tag: 'private' })
  })

  it.each(['"friends"', '"PRIVATE"', '""', 'true', 'friends', 'privateish'])(
    'rejects an invalid audience value %s', (value) => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
      expect(() => generateJsonString(`audience:${value}`)).toThrow()
    }
  )
})
