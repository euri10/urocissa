import { IsolationId } from '@type/types'
import { generateJsonString } from '@/script/lexer/generateJson'
import { defineStore } from 'pinia'
import { LocationQueryValue } from 'vue-router'

function containsAudience(expression: unknown): boolean {
  if (typeof expression !== 'object' || expression === null) return false
  if ('Audience' in expression) return true
  return Object.values(expression).some((value: unknown) =>
    Array.isArray(value) ? value.some(containsAudience) : containsAudience(value)
  )
}

export const useFilterStore = (isolationId: IsolationId) =>
  defineStore('filterStore' + isolationId, {
    state: (): {
      // Records the gallery search filter
      searchString: LocationQueryValue | LocationQueryValue[] | undefined
    } => ({
      searchString: null
    }),
    getters: {
      hasAudienceFilter: (state): boolean => {
        if (typeof state.searchString !== 'string') return false
        try {
          return containsAudience(JSON.parse(generateJsonString(state.searchString)))
        } catch {
          // Invalid expressions use the ordinary keyword-search fallback below.
          return false
        }
      }
    },
    actions: {
      // Generates the filter JSON string using basicString and searchString
      // This JSON info is used to send to the backend
      generateFilterJsonString(basicString: string | null): string | null {
        const hasBasicString = typeof basicString === 'string'
        const searchStringStr = typeof this.searchString === 'string' ? this.searchString : null
        const hasSearchString = searchStringStr !== null
        const stripQuotes = (s: string) => s.replace(/"/g, '')

        if (!hasBasicString && !hasSearchString) return null

        if (hasBasicString && !hasSearchString) {
          try {
            return generateJsonString(basicString)
          } catch (err) {
            console.error(err)
            return null
          }
        }

        if (!hasBasicString && hasSearchString) {
          try {
            return generateJsonString(searchStringStr)
          } catch {
            const s = stripQuotes(searchStringStr)
            return generateJsonString(`any: "${s}"`)
          }
        }

        if (hasBasicString && hasSearchString) {
          try {
            return generateJsonString(`and(${basicString},${searchStringStr})`)
          } catch {
            const s = stripQuotes(searchStringStr)
            return generateJsonString(`and(${basicString}, any: "${s}")`)
          }
        }

        return null
      }
    }
  })()
