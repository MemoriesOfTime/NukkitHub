import {existsSync, readdirSync, readFileSync} from 'node:fs'
import {join} from 'node:path'

import type AllayIndex from '~/types/allayhub-index'

interface LandingHomeData {
  showcase: AllayIndex.PluginSummary[]
  searchResults: AllayIndex.PluginSummary[]
}

const INDEX_DIR = 'NukkitHubIndex'
const SHOWCASE_LIMIT = 40
const SEARCH_RESULTS_LIMIT = 5

let cachedLandingHomeData: LandingHomeData | null = null

interface RankedPluginSummary {
  summary: AllayIndex.PluginSummary
  downloads: number
  stars: number
  updatedAt: number
}

function getDefaultLandingHomeData(): LandingHomeData {
  return {
    showcase: [],
    searchResults: [],
  }
}

function isTemplatePlaceholder(value: unknown): value is string {
  if (typeof value !== 'string') return false
  const text = value.trim()
  return /^\$\{[^}]+\}$/.test(text) || /^@[^@\r\n]+@$/.test(text)
}

function getRepoOwnerFromId(id: string): string {
  const [owner] = id.split('/')
  return owner || id
}

function getRepoNameFromId(id: string): string {
  const [, repo] = id.split('/')
  return repo || id
}

function normalizeName(id: string, name: unknown): string {
  const value = typeof name === 'string' ? name.trim() : ''
  if (!value || isTemplatePlaceholder(value)) {
    return getRepoNameFromId(id) || 'Unknown Plugin'
  }
  return value
}

function normalizeSummary(summary: unknown): string {
  const value = typeof summary === 'string' ? summary.trim() : ''
  return isTemplatePlaceholder(value) ? '' : value
}

function normalizeAuthor(
  id: string,
  authors: AllayIndex.Author[] | undefined,
): string {
  const value = authors?.[0]?.name?.trim() || ''
  if (!value || isTemplatePlaceholder(value)) {
    return getRepoOwnerFromId(id)
  }
  return value
}

function getLicenseType(
  licenseId: string | null | undefined,
): 'open-source' | 'closed-source' {
  return !licenseId || licenseId === 'ARR' ? 'closed-source' : 'open-source'
}

function toIsoString(seconds: number): string {
  return new Date(seconds * 1000).toISOString()
}

function toRankedPluginSummary(plugin: AllayIndex.Plugin): RankedPluginSummary {
  const downloads = Number(plugin.downloads || 0)
  const stars = Number(plugin.stars || 0)
  const updatedAt = Number(plugin.updated_at || 0)

  return {
    downloads,
    stars,
    updatedAt,
    summary: {
      id: plugin.id,
      name: normalizeName(plugin.id, plugin.name),
      summary: normalizeSummary(plugin.summary),
      author: normalizeAuthor(plugin.id, plugin.authors),
      categories: Array.isArray(plugin.categories) ? plugin.categories : [],
      targets: Array.isArray(plugin.targets) ? plugin.targets : [],
      primary_target: plugin.primary_target || undefined,
      api_version: plugin.api_version || '',
      license: getLicenseType(plugin.license?.id),
      icon_url: plugin.icon_url || undefined,
      gallery_image: plugin.gallery?.[0]?.url || undefined,
      downloads,
      stars,
      created_at: toIsoString(Number(plugin.created_at || 0)),
      updated_at: toIsoString(updatedAt),
    },
  }
}

function compareByDownloads(
  left: RankedPluginSummary,
  right: RankedPluginSummary,
): number {
  return (
    right.downloads - left.downloads ||
    right.stars - left.stars ||
    right.updatedAt - left.updatedAt
  )
}

function compareByStars(
  left: RankedPluginSummary,
  right: RankedPluginSummary,
): number {
  return (
    right.stars - left.stars ||
    right.downloads - left.downloads ||
    right.updatedAt - left.updatedAt
  )
}

function getRankedPlugins(): RankedPluginSummary[] {
  if (!existsSync(INDEX_DIR)) {
    return []
  }

  const rankedPlugins: RankedPluginSummary[] = []
  const stack = [INDEX_DIR]

  while (stack.length > 0) {
    const currentDir = stack.pop()
    if (!currentDir) continue

    for (const entry of readdirSync(currentDir, { withFileTypes: true })) {
      const fullPath = join(currentDir, entry.name)

      if (entry.isDirectory()) {
        stack.push(fullPath)
        continue
      }

      if (!entry.isFile() || !entry.name.endsWith('.json')) {
        continue
      }

      const content = readFileSync(fullPath, 'utf8')
      const plugin = JSON.parse(content) as AllayIndex.Plugin
      rankedPlugins.push(toRankedPluginSummary(plugin))
    }
  }

  rankedPlugins.sort(compareByDownloads)

  return rankedPlugins
}

export function getLandingHomeData(): LandingHomeData {
  if (cachedLandingHomeData) {
    return cachedLandingHomeData
  }

  const rankedPlugins = getRankedPlugins()
  if (rankedPlugins.length === 0) {
    cachedLandingHomeData = getDefaultLandingHomeData()
    return cachedLandingHomeData
  }

  const searchRankedPlugins = [...rankedPlugins].sort(compareByStars)

  cachedLandingHomeData = {
    showcase: rankedPlugins
      .slice(0, SHOWCASE_LIMIT)
      .map((plugin) => plugin.summary),
    searchResults: searchRankedPlugins
      .slice(0, SEARCH_RESULTS_LIMIT)
      .map((plugin) => plugin.summary),
  }

  return cachedLandingHomeData
}
