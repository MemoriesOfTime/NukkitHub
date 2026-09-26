import { existsSync, readFileSync } from 'node:fs'

import { search as oramaSearch } from '@orama/orama'
import { restore } from '@orama/plugin-data-persistence'

import type { PluginDocument, PluginSearchDB } from '~/composables/orama-loader'
import type AllayIndex from '~/types/allayhub-index'
import { toPluginSummary } from '~/utils/plugin-summary'

export interface DiscoverInitialData {
  results: AllayIndex.PluginSummary[]
  count: number
}

const INDEX_BIN = 'src/public/orama-index.bin'
const DEFAULT_PER_PAGE = 20

let cachedDiscoverData: DiscoverInitialData | null = null

function getEmptyDiscoverData(): DiscoverInitialData {
  return { results: [], count: 0 }
}

/**
 * Prerendered first page for the default discover view (no filters, first
 * page, sorted by stars). Restores the same seqproto index the browser
 * downloads and runs the same query searchPlugins() would run, so the
 * prerendered order matches client-side search results exactly, including
 * ties (orama resolves equal sort keys by insertion order, which cannot be
 * reproduced by re-sorting the JSON source).
 *
 * Returns empty data on any failure: this only accelerates the first paint,
 * so a broken index must degrade to the client-side search path instead of
 * failing the prerender build.
 */
export async function getDiscoverInitialData(): Promise<DiscoverInitialData> {
  if (cachedDiscoverData) {
    return cachedDiscoverData
  }

  try {
    if (!existsSync(INDEX_BIN)) {
      return getEmptyDiscoverData()
    }

    const file = readFileSync(INDEX_BIN)
    const data = file.buffer.slice(
      file.byteOffset,
      file.byteOffset + file.byteLength,
    ) as ArrayBuffer
    const db = (await restore('seqproto', data, undefined, {
      sort: { enabled: true },
    })) as PluginSearchDB

    const results = await oramaSearch(db, {
      term: '',
      properties: ['name', 'owner'],
      boost: { name: 2, owner: 1 },
      limit: DEFAULT_PER_PAGE,
      offset: 0,
      sortBy: { property: 'stars', order: 'DESC' },
      tolerance: 1,
    })

    cachedDiscoverData = {
      results: results.hits.map((hit) =>
        toPluginSummary(hit.document as PluginDocument),
      ),
      count: results.count,
    }

    return cachedDiscoverData
  } catch (error) {
    console.error('Failed to compute prerendered discover data:', error)
    return getEmptyDiscoverData()
  }
}
