#!/usr/bin/env bun
/**
 * NukkitHub Public API v2 exporter
 *
 * Reads NukkitHubIndex/{owner}/{name}.json files and writes the static API
 * directly into the site's public /api subtree
 * (src/public/api), so `nuxi generate` serves it same-origin at /api/…
 * together with the main site — one origin, one deploy pipeline.
 *
 * Usage:
 *   bun run scripts/export-api.ts [--api-base <url>]
 *
 * The script only uses node/bun built-ins, so CI can run it without
 * installing dependencies. It fails loudly (exit 1) on any inconsistency.
 */

// Loads the Bun global namespace (incl. import.meta.main) for tsc.
/// <reference types="bun-types" />
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { parseArgs } from 'node:util'

import type AllayIndex from '../src/types/allayhub-index'
import type ApiV2 from '../src/types/api-v2'

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const SCHEMA_VERSION = '1.0'
const INDEX_DIR = 'NukkitHubIndex'
/** Output: the site's public /api subtree, bundled by `nuxi generate` */
const PUBLIC_API_DIR = 'src/public/api'
/** Public mount: /api subtree of the shared domain */
const DEFAULT_API_BASE = 'https://plugins.nukkit-mot.com/api'
const SITE_URL = 'https://plugins.nukkit-mot.com'
const DOCS_URL = `${SITE_URL}/api-docs`

const LOADERS: ReadonlyArray<{ name: string; display_name: string }> = [
  { name: 'nkmot', display_name: 'Nukkit-MOT' },
  { name: 'pnx', display_name: 'PowerNukkitX' },
  { name: 'lumi', display_name: 'Lumi' },
  { name: 'nkx', display_name: 'NukkitX' },
]

const CATEGORIES: ReadonlyArray<{ name: string; display_name: string }> = [
  { name: 'adventure', display_name: 'Adventure' },
  { name: 'cursed', display_name: 'Cursed' },
  { name: 'decoration', display_name: 'Decoration' },
  { name: 'economy', display_name: 'Economy' },
  { name: 'equipment', display_name: 'Equipment' },
  { name: 'game-mechanics', display_name: 'Game Mechanics' },
  { name: 'library', display_name: 'Library' },
  { name: 'management', display_name: 'Management' },
  { name: 'minigame', display_name: 'Minigame' },
  { name: 'optimization', display_name: 'Optimization' },
  { name: 'social', display_name: 'Social' },
  { name: 'storage', display_name: 'Storage' },
  { name: 'technology', display_name: 'Technology' },
  { name: 'transportation', display_name: 'Transportation' },
  { name: 'utility', display_name: 'Utility' },
]

const KNOWN_LOADERS = new Set(LOADERS.map((l) => l.name))
const KNOWN_CATEGORIES = new Set(CATEGORIES.map((c) => c.name))

/** id must be exactly "owner/name" with URL- and filename-safe characters */
const ID_RE = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/

function isValidPluginId(id: string): boolean {
  if (!ID_RE.test(id)) return false
  // defense in depth: '.' / '..' path segments must never become directories
  return id.split('/').every((segment) => segment !== '.' && segment !== '..')
}
/** version strings that can be used as a path segment unchanged */
const SAFE_VERSION_RE = /^[A-Za-z0-9][A-Za-z0-9._+-]*$/

class ExportError extends Error {}

function fail(message: string): never {
  throw new ExportError(message)
}

// ---------------------------------------------------------------------------
// Input cleaning (mirrors processPluginData in src/composables/usePlugins.ts)
// ---------------------------------------------------------------------------

export function isTemplatePlaceholder(value: unknown): boolean {
  if (typeof value !== 'string') return false
  const text = value.trim()
  return /^\$\{[^}]+\}$/.test(text) || /^@[^@\r\n]+@$/.test(text)
}

export function cleanPlugin(data: AllayIndex.Plugin): AllayIndex.Plugin {
  const raw = data as unknown as Record<string, unknown>
  const result: Record<string, unknown> = {}

  for (const [key, value] of Object.entries(raw)) {
    // "!key" entries restore a null/undefined sibling (cache fallback format)
    if (key.startsWith('!')) {
      const normalKey = key.slice(1)
      if (
        !(normalKey in raw) ||
        raw[normalKey] === undefined ||
        raw[normalKey] === null
      ) {
        result[normalKey] = value
      }
      continue
    }
    result[key] = value
  }

  const id = typeof result.id === 'string' ? result.id : ''
  const fallbackName = id.split('/')[1] || 'Unknown Plugin'
  const name = typeof result.name === 'string' ? result.name.trim() : ''
  if (!name || isTemplatePlaceholder(name)) {
    result.name = fallbackName
  }

  const summary =
    typeof result.summary === 'string' ? result.summary.trim() : ''
  if (isTemplatePlaceholder(summary)) {
    result.summary = ''
  }

  return result as unknown as AllayIndex.Plugin
}

// ---------------------------------------------------------------------------
// Pure mapping helpers (exported for tests)
// ---------------------------------------------------------------------------

/** Unix seconds → ISO 8601 UTC string; invalid input becomes the epoch */
export function toIso(unix: unknown): string {
  const n = typeof unix === 'number' && Number.isFinite(unix) ? unix : 0
  return new Date(Math.floor(n) * 1000).toISOString()
}

function str(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

/**
 * Keep only absolute http(s) URLs; anything else is dropped. Index content
 * (README-derived URLs, icon URLs) is third-party data — a `javascript:`
 * value must never survive into the API for consumers to render as a link.
 */
function safeUrl(value: unknown): string {
  const s = str(value)
  if (!s) return ''
  try {
    const parsed = new URL(s)
    return parsed.protocol === 'https:' || parsed.protocol === 'http:' ? s : ''
  } catch {
    return ''
  }
}

function num(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}

/**
 * Filename-safe form of a version string. Safe strings pass through
 * unchanged; otherwise every unsafe character becomes '-', and a leading
 * non-alphanumeric is prefixed with 'v'.
 */
export function toVersionSlug(version: string): string {
  if (SAFE_VERSION_RE.test(version)) return version
  let cleaned = version.replace(/[^A-Za-z0-9._+-]/g, '-')
  if (!/^[A-Za-z0-9]/.test(cleaned)) cleaned = `v${cleaned}`
  if (!SAFE_VERSION_RE.test(cleaned)) return `v-${cleaned.replace(/-/g, '_')}`
  return cleaned
}

/**
 * Globally-unique version id: "owner/name@version_number". The "@"
 * separator can appear in neither project id segments nor version slugs, so
 * ids parse unambiguously — this is what /v2/version/{version_id} accepts.
 */
export function toVersionId(projectId: string, versionSlug: string): string {
  return `${projectId}@${versionSlug}`
}

export interface SlugAssignment {
  /** original version string → unique path-safe slug */
  slugByVersion: Map<string, string>
}

/**
 * Assign unique slugs for a plugin's versions; collisions get -2, -3, …
 * suffixes (deterministic by list order, which is newest-first).
 */
export function resolveVersionSlugs(
  versions: readonly { version: string }[],
): SlugAssignment {
  const slugByVersion = new Map<string, string>()
  const taken = new Set<string>()
  for (const v of versions) {
    // duplicate version strings keep their first assignment (idempotent)
    if (slugByVersion.has(v.version)) continue
    const base = toVersionSlug(v.version)
    let slug = base
    let counter = 2
    while (taken.has(slug.toLowerCase())) {
      slug = `${base}-${counter}`
      counter += 1
    }
    taken.add(slug.toLowerCase())
    slugByVersion.set(v.version, slug)
  }
  return { slugByVersion }
}

/** Newest first; ties broken by version string descending (deterministic) */
export function sortVersionsDesc<
  T extends { published_at: number; version: string },
>(versions: readonly T[]): T[] {
  return [...versions].sort((a, b) => {
    if (b.published_at !== a.published_at)
      return b.published_at - a.published_at
    return b.version.localeCompare(a.version)
  })
}

/** First non-prerelease version, falling back to the newest one */
export function pickLatestVersion<T extends { prerelease: boolean }>(
  sortedDesc: readonly T[],
): T | undefined {
  return sortedDesc.find((v) => !v.prerelease) ?? sortedDesc[0]
}

function githubAppendix(source: string, appendix: string): string {
  if (!source) return ''
  return source.replace(/\/+$/, '') + appendix
}

function mapDependency(dep: AllayIndex.Dependency): ApiV2.Dependency {
  return {
    plugin_id: str(dep?.plugin_id),
    version_range: str(dep?.version_range),
    dependency_type:
      dep?.dependency_type === 'required' ? 'required' : 'optional',
  }
}

function mapGalleryItem(item: AllayIndex.GalleryImage): ApiV2.GalleryImage {
  return {
    url: safeUrl(item?.url),
    title: str(item?.title),
    description: str(item?.description),
    created: str(item?.created),
  }
}

/**
 * files[].hashes from the index file's sha256. The indexer only emits
 * well-formed digests, but the source is a JSON file on disk — malformed
 * values degrade to {} instead of poisoning the output.
 */
export function fileHashes(sha256: unknown): ApiV2.VersionFile['hashes'] {
  if (typeof sha256 !== 'string') return {}
  const hex = sha256.toLowerCase()
  return /^[0-9a-f]{64}$/.test(hex) ? { sha256: hex } : {}
}

export function mapVersion(
  raw: AllayIndex.RawVersion,
  slug: string,
  plugin: AllayIndex.Plugin,
): ApiV2.Version {
  const projectId = str(plugin.id)
  return {
    id: toVersionId(projectId, slug),
    project_id: projectId,
    name: str(raw.name),
    version_number: slug,
    changelog: str(raw.changelog),
    version_type: raw.prerelease ? 'beta' : 'release',
    date_published: toIso(raw.published_at),
    downloads: num(raw.downloads),
    loaders: (plugin.targets ?? []).map(str),
    game_versions: [str(plugin.api_version)].filter(Boolean),
    files: (raw.files ?? []).map((f) => ({
      url: safeUrl(f?.url),
      filename: str(f?.filename),
      primary: f?.primary === true,
      size: num(f?.size),
      hashes: fileHashes(f?.sha256),
    })),
    dependencies: (plugin.dependencies ?? []).map(mapDependency),
  }
}

/**
 * Modrinth-shaped search hit. `versions` carries globally-unique version ids
 * ("owner/name@version_number", Modrinth semantics); `latest_version` stays
 * the path-safe version number of the /project/{id}/version/{n} endpoint.
 */
export function mapSearchHit(
  plugin: AllayIndex.Plugin,
  slugByVersion?: ReadonlyMap<string, string>,
): ApiV2.SearchHit {
  const sorted = sortVersionsDesc(plugin.versions ?? [])
  const projectId = str(plugin.id)
  const slugOf = (version: string) =>
    slugByVersion?.get(version) ?? toVersionSlug(version)
  const latest = pickLatestVersion(sorted)
  return {
    project_id: projectId,
    project_type: 'plugin',
    slug: projectId.split('/')[1] ?? '',
    author: str(plugin.authors?.[0]?.name),
    title: str(plugin.name),
    description: str(plugin.summary),
    categories: (plugin.categories ?? []).map(str),
    loaders: (plugin.targets ?? []).map(str),
    game_versions: [str(plugin.api_version)].filter(Boolean),
    versions: sorted.map((v) => toVersionId(projectId, slugOf(v.version))),
    downloads: num(plugin.downloads),
    icon_url: safeUrl(plugin.icon_url),
    date_created: toIso(plugin.created_at),
    date_modified: toIso(plugin.updated_at),
    latest_version: latest ? slugOf(latest.version) : '',
    license: {
      id: str(plugin.license?.id),
      name: str(plugin.license?.name),
      url: str(plugin.license?.url),
    },
    stars: num(plugin.stars),
  }
}

export function mapProject(
  plugin: AllayIndex.Plugin,
  sortedVersions: readonly AllayIndex.RawVersion[],
  slugByVersion: ReadonlyMap<string, string>,
): ApiV2.Project {
  const iconUrl = safeUrl(plugin.icon_url)
  const source = safeUrl(plugin.source)
  const projectId = str(plugin.id)
  return {
    id: projectId,
    slug: projectId.split('/')[1] ?? '',
    project_type: 'plugin',
    title: str(plugin.name),
    description: str(plugin.summary),
    body: str(plugin.description),
    categories: (plugin.categories ?? []).map(str),
    loaders: (plugin.targets ?? []).map(str),
    game_versions: [str(plugin.api_version)].filter(Boolean),
    downloads: num(plugin.downloads),
    icon_url: iconUrl,
    raw_icon_url: iconUrl,
    issues_url: githubAppendix(source, '/issues'),
    source_url: source,
    wiki_url: safeUrl(plugin.links?.wiki) || githubAppendix(source, '/wiki'),
    discord_url: safeUrl(plugin.links?.discord),
    published: toIso(plugin.created_at),
    updated: toIso(plugin.updated_at),
    versions: sortedVersions.map((v) =>
      toVersionId(
        projectId,
        slugByVersion.get(v.version) ?? toVersionSlug(v.version),
      ),
    ),
    license: {
      id: str(plugin.license?.id),
      name: str(plugin.license?.name),
      url: str(plugin.license?.url),
    },
    gallery: (plugin.gallery ?? []).map(mapGalleryItem),
    authors: (plugin.authors ?? []).map((a) => ({
      name: str(a?.name),
      url: safeUrl(a?.url),
      avatar_url: safeUrl(a?.avatar_url),
    })),
    stars: num(plugin.stars),
    dependencies: (plugin.dependencies ?? []).map(mapDependency),
    homepage_url: safeUrl(plugin.links?.homepage),
    api_version: str(plugin.api_version),
    server_version: str(plugin.server_version),
  }
}

// ---------------------------------------------------------------------------
// IO helpers
// ---------------------------------------------------------------------------

interface OutputFile {
  relPath: string
  content: string
}

/**
 * Serialize with defensive HTML escaping: even if a host mis-serves the
 * file as HTML, embedded "<script>" text in plugin descriptions/changelogs
 * cannot break out of the JSON string context.
 */
function jsonContent(value: unknown): string {
  return JSON.stringify(value)
    .replace(/</g, '\\u003c')
    .replace(/>/g, '\\u003e')
    .replace(/&/g, '\\u0026')
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function readIndexPlugins(): AllayIndex.Plugin[] {
  const root = resolve(process.cwd(), INDEX_DIR)
  if (!existsSync(root)) {
    fail(
      `${INDEX_DIR}/ not found — run this from the repository root after checking out the index (see deploy docs)`,
    )
  }
  const plugins: AllayIndex.Plugin[] = []
  const seenIds = new Map<string, string>() // lowercase id → source file
  for (const owner of readdirSync(root, { withFileTypes: true })) {
    if (!owner.isDirectory()) continue
    const ownerDir = join(root, owner.name)
    for (const entry of readdirSync(ownerDir, { withFileTypes: true })) {
      if (!entry.isFile() || !entry.name.endsWith('.json')) continue
      const file = join(ownerDir, entry.name)
      let data: unknown
      try {
        data = JSON.parse(readFileSync(file, 'utf8'))
      } catch (err) {
        fail(`Cannot parse ${file}: ${(err as Error).message}`)
      }
      const raw = data as Record<string, unknown>
      if (typeof raw.id !== 'string' || !isValidPluginId(raw.id)) {
        fail(`Invalid or missing "id" in ${file}: ${String(raw.id)}`)
      }
      const dupSource = seenIds.get(raw.id.toLowerCase())
      if (dupSource) {
        fail(
          `Duplicate plugin id "${raw.id}" (case-insensitive) in ${file} — already seen in ${dupSource}`,
        )
      }
      seenIds.set(raw.id.toLowerCase(), file)
      if (!Array.isArray(raw.versions)) {
        fail(`Invalid "versions" (expected array) in ${file}`)
      }
      for (const v of raw.versions as unknown[]) {
        const ver = v as Record<string, unknown>
        if (typeof ver?.version !== 'string' || ver.version === '') {
          fail(
            `Invalid version entry (missing non-empty "version") in ${file}: ${JSON.stringify(ver).slice(0, 120)}`,
          )
        }
        if (
          typeof ver?.published_at !== 'number' ||
          !Number.isFinite(ver.published_at)
        ) {
          fail(`Version "${ver.version}" in ${file} has invalid "published_at"`)
        }
        if (ver.files !== undefined && !Array.isArray(ver.files)) {
          fail(`Version "${ver.version}" in ${file} has invalid "files"`)
        }
        for (const f of (ver.files ?? []) as unknown[]) {
          const file_ = f as Record<string, unknown>
          if (
            typeof file_?.url !== 'string' ||
            typeof file_?.filename !== 'string'
          ) {
            fail(
              `Version "${ver.version}" in ${file} has a file entry without url/filename`,
            )
          }
        }
      }
      plugins.push(cleanPlugin(raw as unknown as AllayIndex.Plugin))
    }
  }
  if (plugins.length === 0) {
    fail(`No plugin JSON files found under ${INDEX_DIR}/`)
  }
  return plugins
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

interface BuildStats {
  plugins: number
  pluginsWithDownloads: number
  versions: number
  releaseFiles: number
  filesWithHashes: number
  /** Distinct sha256 values (version_file/ emits one file per distinct hash) */
  hashLookupFiles: number
  byLoader: Record<string, number>
  fileCount: number
  searchBytes: number
}

/** Pure in-memory build: plugins → output files + stats (exported for tests) */
export function buildFiles(
  plugins: readonly AllayIndex.Plugin[],
  apiBase: string,
): { files: OutputFile[]; stats: BuildStats } {
  const files: OutputFile[] = []
  const caseInsensitivePaths = new Map<string, string>() // normalized → relPath
  const add = (relPath: string, content: string) => {
    const normalized = relPath.toLowerCase()
    const existing = caseInsensitivePaths.get(normalized)
    if (existing) {
      fail(`Case-insensitive path collision: "${relPath}" vs "${existing}"`)
    }
    caseInsensitivePaths.set(normalized, relPath)
    files.push({ relPath, content })
  }
  const addJson = (relPath: string, value: unknown) => {
    add(relPath, jsonContent(value))
  }

  // ---- per-plugin files -------------------------------------------------
  interface Prepared {
    plugin: AllayIndex.Plugin
    sorted: AllayIndex.RawVersion[]
    slugs: SlugAssignment
    mapped: ApiV2.Version[]
    hit: ApiV2.SearchHit
    project: ApiV2.Project
    latest?: ApiV2.Version
  }
  const prepared: Prepared[] = plugins.map((plugin) => {
    const sorted = sortVersionsDesc(plugin.versions ?? [])
    const slugs = resolveVersionSlugs(sorted)
    const mapped = sorted.map((v) =>
      mapVersion(
        v,
        slugs.slugByVersion.get(v.version) ?? toVersionSlug(v.version),
        plugin,
      ),
    )
    const latestRaw = pickLatestVersion(sorted)
    return {
      plugin,
      sorted,
      slugs,
      mapped,
      hit: mapSearchHit(plugin, slugs.slugByVersion),
      project: mapProject(plugin, sorted, slugs.slugByVersion),
      latest: latestRaw
        ? mapped.find(
            (m) =>
              m.version_number === slugs.slugByVersion.get(latestRaw.version),
          )
        : undefined,
    }
  })

  // ---- per-plugin files ---------------------------------------------------
  for (const p of prepared) {
    const [owner, name] = p.plugin.id.split('/')
    const base = `v2/project/${owner}/${name}`
    addJson(`${base}.json`, p.project)
    addJson(`${base}/version.json`, p.mapped)
    for (const v of p.mapped) {
      addJson(`${base}/version/${v.version_number}.json`, v)
    }
    if (p.latest) addJson(`${base}/latest.json`, p.latest)
  }

  // ---- search files -----------------------------------------------------
  const byRecency = [...prepared].sort((a, b) => {
    const aTime = a.plugin.updated_at || 0
    const bTime = b.plugin.updated_at || 0
    if (bTime !== aTime) return bTime - aTime
    return a.plugin.id.localeCompare(b.plugin.id)
  })

  // ---- per-hash reverse-lookup files -------------------------------------
  // v2/version_file/{sha256}.json carries the same body as the owning
  // version (Modrinth's /version_file/{hash} returns the version). The same
  // asset re-uploaded across releases or repos resolves deterministically to
  // the newest version (byRecency order here, sorted desc within a plugin —
  // never the index directory's filesystem order, which would make the
  // output non-reproducible across machines).
  let filesWithHashes = 0
  for (const p of prepared) {
    for (const v of p.mapped) {
      for (const f of v.files) {
        if (f.hashes.sha256) filesWithHashes += 1
      }
    }
  }
  const hashBodies = new Map<string, string>()
  for (const p of byRecency) {
    for (const v of p.mapped) {
      for (const f of v.files) {
        const sha256 = f.hashes.sha256
        if (sha256 && !hashBodies.has(sha256)) {
          hashBodies.set(sha256, jsonContent(v))
        }
      }
    }
  }
  for (const [sha256, body] of hashBodies) {
    add(`v2/version_file/${sha256}.json`, body)
  }

  const searchResponse = (hits: ApiV2.SearchHit[]): ApiV2.SearchResponse => ({
    offset: 0,
    limit: hits.length,
    total_hits: hits.length,
    hits,
  })
  const searchJson = jsonContent(searchResponse(byRecency.map((p) => p.hit)))
  add('v2/search.json', searchJson)
  for (const loader of LOADERS) {
    const hits = byRecency
      .filter((p) => p.plugin.targets?.includes(loader.name))
      .map((p) => p.hit)
    add(`v2/search/${loader.name}.json`, jsonContent(searchResponse(hits)))
  }

  // ---- tags ---------------------------------------------------------------
  addJson(
    'v2/tag/loader.json',
    LOADERS.map((l) => ({
      icon: '',
      name: l.name,
      display_name: l.display_name,
    })),
  )
  addJson(
    'v2/tag/category.json',
    CATEGORIES.map((c) => ({ icon: '', name: c.name, project_type: 'plugin' })),
  )
  const gameVersions = new Set<string>()
  for (const p of prepared) {
    if (p.plugin.api_version) gameVersions.add(p.plugin.api_version)
  }
  addJson(
    'v2/tag/game_version.json',
    [...gameVersions]
      .sort((a, b) => compareVersionStrings(a, b))
      .map((v) => ({ version: v, date: '' })),
  )

  // ---- meta ---------------------------------------------------------------
  const byLoader: Record<string, number> = {}
  let releaseFiles = 0
  let versionCount = 0
  let withDownloads = 0
  for (const p of prepared) {
    if (p.sorted.length > 0) withDownloads += 1
    versionCount += p.sorted.length
    releaseFiles += p.sorted.reduce((sum, v) => sum + (v.files?.length ?? 0), 0)
    for (const target of p.plugin.targets ?? []) {
      if (KNOWN_LOADERS.has(target))
        byLoader[target] = (byLoader[target] ?? 0) + 1
    }
  }
  const meta: ApiV2.Meta = {
    schema_version: SCHEMA_VERSION,
    generated_at: new Date().toISOString(),
    api_base: apiBase,
    site_url: SITE_URL,
    docs_url: DOCS_URL,
    counts: {
      plugins: prepared.length,
      plugins_with_downloads: withDownloads,
      versions: versionCount,
      release_files: releaseFiles,
      files_with_hashes: filesWithHashes,
      by_loader: Object.fromEntries(
        LOADERS.map((l) => [l.name, byLoader[l.name] ?? 0]),
      ),
    },
    deprecated_fields: {},
  }
  addJson('v2/meta.json', meta)

  // ---- landing page (GitHub Pages serves it for the /api/ directory URL) --
  add('index.html', renderIndexPage(apiBase))

  return {
    files,
    stats: {
      plugins: prepared.length,
      pluginsWithDownloads: withDownloads,
      versions: versionCount,
      releaseFiles,
      filesWithHashes,
      hashLookupFiles: hashBodies.size,
      byLoader,
      fileCount: files.length,
      searchBytes: Buffer.byteLength(searchJson),
    },
  }
}

/** Numeric-aware ascending sort for dot-separated version strings */
export function compareVersionStrings(a: string, b: string): number {
  const pa = a.split('.').map((x) => Number.parseInt(x, 10))
  const pb = b.split('.').map((x) => Number.parseInt(x, 10))
  const len = Math.max(pa.length, pb.length)
  for (let i = 0; i < len; i++) {
    const na = Number.isFinite(pa[i] as number) ? (pa[i] as number) : 0
    const nb = Number.isFinite(pb[i] as number) ? (pb[i] as number) : 0
    if (na !== nb) return na - nb
  }
  return a.localeCompare(b)
}

function renderIndexPage(apiBase: string): string {
  const base = escapeHtml(apiBase)
  const endpoints: Array<[string, string]> = [
    [
      'v2/search (dynamic)',
      'Modrinth-style dynamic search: query / facets / index / offset / limit',
    ],
    [
      'v2/projects?ids= (dynamic)',
      'Batch project lookup, up to 20 ids (Modrinth syntax)',
    ],
    [
      'v2/version/{owner}/{name}@{version_number} (dynamic)',
      'Single version by globally-unique version id',
    ],
    [
      'v2/versions?ids= (dynamic)',
      'Batch version lookup by version ids, up to 20 (Modrinth syntax)',
    ],
    [
      'v2/version_file/{sha256} (dynamic)',
      'Version owning the file with this hash (sha256; unknown hashes 404)',
    ],
    [
      'v2/project/{owner}/{name} (dynamic)',
      'Modrinth-style suffixless routes: /version, /version/{n}, /latest (also single-segment slugs)',
    ],
    ['v2/search.json', 'All plugins in one Modrinth-style search response'],
    [
      'v2/search/{loader}.json',
      'Pre-filtered search (nkmot / pnx / lumi / nkx)',
    ],
    ['v2/project/{owner}/{name}.json', 'Project detail with versions'],
    ['v2/project/{owner}/{name}/version.json', 'Version list (bare array)'],
    [
      'v2/project/{owner}/{name}/version/{version_number}.json',
      'Single version with files',
    ],
    ['v2/project/{owner}/{name}/latest.json', 'Latest installable version'],
    ['v2/tag/loader.json', 'Loader tags'],
    ['v2/tag/category.json', 'Category tags'],
    ['v2/tag/game_version.json', 'Server API version tags'],
    ['v2/meta.json', 'Index metadata, counts, canonical api_base'],
  ]
  const rows = endpoints
    .map(([path, desc]) => {
      // template endpoints (containing "{") have no concrete file to link
      const link = path.includes('{')
        ? escapeHtml(path)
        : `<a href="./${escapeHtml(path)}">${escapeHtml(path)}</a>`
      return `<tr><td>${link}</td><td>${escapeHtml(desc)}</td></tr>`
    })
    .join('\n')
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>NukkitHub API</title>
<style>
  body { font: 16px/1.6 system-ui, sans-serif; max-width: 52rem; margin: 2rem auto; padding: 0 1rem; color: #1f2937; }
  h1 { font-size: 1.5rem; }
  code { background: #f3f4f6; padding: .1rem .35rem; border-radius: .25rem; font-size: .9em; }
  table { border-collapse: collapse; width: 100%; margin: 1rem 0; }
  th, td { text-align: left; padding: .4rem .6rem; border-bottom: 1px solid #e5e7eb; vertical-align: top; }
  a { color: #06c; }
  .note { color: #6b7280; font-size: .9rem; }
</style>
</head>
<body>
<h1>NukkitHub Public API (v2)</h1>
<p>Read-only JSON API for the NukkitHub plugin index, shaped after the
<a href="https://docs.modrinth.com/api/" rel="noopener">Modrinth API v2</a>.
This deployment's canonical base is <code>${base}</code>.</p>
<table>
<thead><tr><th>Endpoint</th><th>Description</th></tr></thead>
<tbody>
${rows}
</tbody>
</table>
<p class="note">The suffixless routes take query parameters and single-segment
slugs; the .json files return the same shapes as plain files — search.json is
exactly a parameterless /v2/search response. Remaining differences from
Modrinth (two-segment ids, no hash lookups, no auth) are documented at
<a href="${escapeHtml(DOCS_URL)}" rel="noopener">${escapeHtml(DOCS_URL)}</a>.</p>
</body>
</html>
`
}

// ---------------------------------------------------------------------------
// Verify (read-back self-check)
// ---------------------------------------------------------------------------

function verify(
  outDir: string,
  plugins: readonly AllayIndex.Plugin[],
  stats: BuildStats,
): void {
  const errors: string[] = []
  const warnings: string[] = []
  const warn = (msg: string) => {
    if (warnings.length < 20) warnings.push(msg)
  }

  // unknown vocabulary (data-quality notes, not failures)
  for (const plugin of plugins) {
    for (const target of plugin.targets ?? []) {
      if (!KNOWN_LOADERS.has(target))
        warn(`${plugin.id}: unknown loader "${target}"`)
    }
    for (const category of plugin.categories ?? []) {
      if (!KNOWN_CATEGORIES.has(category))
        warn(`${plugin.id}: unknown category "${category}"`)
    }
  }

  // file count
  const counted = countFiles(outDir)
  if (counted !== stats.fileCount) {
    errors.push(
      `file count mismatch: wrote ${stats.fileCount}, found ${counted} on disk`,
    )
  }

  // meta counts
  const meta = JSON.parse(
    readFileSync(join(outDir, 'v2/meta.json'), 'utf8'),
  ) as ApiV2.Meta
  if (meta.counts.plugins !== stats.plugins)
    errors.push('meta.plugins mismatch')
  if (meta.counts.versions !== stats.versions)
    errors.push('meta.versions mismatch')
  if (meta.counts.release_files !== stats.releaseFiles)
    errors.push('meta.release_files mismatch')
  if (meta.counts.files_with_hashes !== stats.filesWithHashes) {
    errors.push('meta.files_with_hashes mismatch')
  }
  if (meta.counts.plugins_with_downloads !== stats.pluginsWithDownloads) {
    errors.push('meta.plugins_with_downloads mismatch')
  }

  // version_file/{sha256}.json: each lookup file must parse as the version
  // that owns the hash (the guarantee the /version_file/{hash} endpoint
  // gives consumers), and the count must match the distinct-hash stats
  const versionFileDir = join(outDir, 'v2/version_file')
  const hashFiles = existsSync(versionFileDir)
    ? readdirSync(versionFileDir).filter((f) => f.endsWith('.json'))
    : []
  if (hashFiles.length !== stats.hashLookupFiles) {
    errors.push(
      `version_file count mismatch: wrote ${stats.hashLookupFiles}, found ${hashFiles.length} on disk`,
    )
  }
  let recountedFilesWithHashes = 0
  for (const hashFile of hashFiles) {
    const sha256 = hashFile.replace(/\.json$/, '')
    if (!/^[0-9a-f]{64}$/.test(sha256)) {
      errors.push(`version_file: malformed hash filename "${hashFile}"`)
      continue
    }
    const body = JSON.parse(
      readFileSync(join(versionFileDir, hashFile), 'utf8'),
    ) as ApiV2.Version
    if (!body.files.some((f) => f.hashes.sha256 === sha256)) {
      errors.push(
        `version_file/${sha256}: body does not contain the hashed file`,
      )
    }
  }
  for (const plugin of plugins) {
    for (const version of plugin.versions ?? []) {
      for (const file of version.files ?? []) {
        if (fileHashes(file.sha256).sha256) recountedFilesWithHashes += 1
        else if (file.sha256)
          warn(`${plugin.id}: malformed sha256 for "${file.filename}"`)
      }
    }
  }
  if (recountedFilesWithHashes !== stats.filesWithHashes) {
    errors.push('files_with_hashes recount mismatch')
  }

  // search envelope
  const search = JSON.parse(
    readFileSync(join(outDir, 'v2/search.json'), 'utf8'),
  ) as ApiV2.SearchResponse
  if (
    search.total_hits !== stats.plugins ||
    search.hits.length !== stats.plugins
  ) {
    errors.push(
      `search.json envelope mismatch (total_hits=${search.total_hits}, hits=${search.hits.length}, plugins=${stats.plugins})`,
    )
  }
  for (let i = 1; i < search.hits.length; i++) {
    if (search.hits[i - 1].date_modified < search.hits[i].date_modified) {
      errors.push(
        `search.json not sorted by date_modified desc at hit ${i} (${search.hits[i].project_id})`,
      )
      break
    }
  }

  // per-plugin: latest consistency, zero-release handling, search-hit slug consistency
  const versionNumbersByPlugin = new Map<string, Set<string>>()
  for (const plugin of plugins) {
    const [owner, name] = plugin.id.split('/')
    const base = join(outDir, 'v2/project', owner, name)
    const versionList = JSON.parse(
      readFileSync(join(base, 'version.json'), 'utf8'),
    ) as ApiV2.Version[]
    const versionNumbers = new Set(versionList.map((v) => v.version_number))
    versionNumbersByPlugin.set(plugin.id, versionNumbers)
    if (versionNumbers.size !== versionList.length) {
      errors.push(`${plugin.id}: duplicate version_number in version.json`)
    }
    for (const v of versionList) {
      if (v.id !== `${plugin.id}@${v.version_number}`) {
        errors.push(
          `${plugin.id}: version id "${v.id}" does not match "${plugin.id}@${v.version_number}"`,
        )
      }
      if (v.project_id !== plugin.id) {
        errors.push(
          `${plugin.id}@${v.version_number}: project_id mismatch ("${v.project_id}")`,
        )
      }
    }
    const detail = JSON.parse(
      readFileSync(`${base}.json`, 'utf8'),
    ) as ApiV2.Project
    for (const versionId of detail.versions) {
      const sep = versionId.indexOf('@')
      if (
        sep < 0 ||
        versionId.slice(0, sep) !== plugin.id ||
        !versionNumbers.has(versionId.slice(sep + 1))
      ) {
        errors.push(
          `${plugin.id}: project versions entry "${versionId}" does not resolve`,
        )
      }
    }
    const hasReleases = (plugin.versions ?? []).length > 0
    const latestPath = join(base, 'latest.json')
    if (hasReleases && !existsSync(latestPath)) {
      errors.push(`${plugin.id}: has releases but no latest.json`)
    }
    if (!hasReleases && existsSync(latestPath)) {
      errors.push(`${plugin.id}: no releases but latest.json exists`)
    }
    if (hasReleases) {
      const latest = JSON.parse(
        readFileSync(latestPath, 'utf8'),
      ) as ApiV2.Version
      if (!versionNumbers.has(latest.version_number)) {
        errors.push(
          `${plugin.id}: latest.json version "${latest.version_number}" not in version.json`,
        )
      }
      // primary flag: data-quality note only (indexer should mark one)
      if (latest.files.length > 0 && !latest.files.some((f) => f.primary)) {
        warn(`${plugin.id}: latest version has files but none flagged primary`)
      }
      for (const v of versionList) {
        for (const f of v.files) {
          try {
            const parsed = new URL(f.url)
            if (parsed.protocol !== 'https:') {
              errors.push(
                `${plugin.id}@${v.version_number}: non-https file url "${f.url}"`,
              )
            }
          } catch {
            errors.push(
              `${plugin.id}@${v.version_number}: unparseable file url "${f.url}"`,
            )
          }
        }
      }
    }
  }

  // every search hit's latest_version must be resolvable through the
  // version endpoint (slug consistency between list and detail files)
  for (const hit of search.hits) {
    const versionNumbers = versionNumbersByPlugin.get(hit.project_id)
    if (!versionNumbers) {
      errors.push(`search hit ${hit.project_id}: no version.json record`)
      continue
    }
    if (hit.latest_version && !versionNumbers.has(hit.latest_version)) {
      errors.push(
        `search hit ${hit.project_id}: latest_version "${hit.latest_version}" not in version.json`,
      )
    }
    for (const versionId of hit.versions) {
      const sep = versionId.indexOf('@')
      if (
        sep < 0 ||
        versionId.slice(0, sep) !== hit.project_id ||
        !versionNumbers.has(versionId.slice(sep + 1))
      ) {
        errors.push(
          `search hit ${hit.project_id}: version id "${versionId}" does not resolve`,
        )
      }
    }
  }

  if (errors.length > 0) {
    fail(
      `verification failed (${errors.length} issue(s)):\n  - ${errors.slice(0, 20).join('\n  - ')}`,
    )
  }
  if (warnings.length > 0) {
    console.warn(
      `verification warnings (${warnings.length}):\n  - ${warnings.join('\n  - ')}`,
    )
  }
}

function countFiles(dir: string): number {
  let n = 0
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) n += countFiles(full)
    else if (entry.isFile()) n += 1
  }
  return n
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main(): void {
  const { values } = parseArgs({
    options: {
      'api-base': { type: 'string', default: DEFAULT_API_BASE },
      help: { type: 'boolean', default: false },
    },
  })
  if (values.help) {
    console.log('Usage: bun run scripts/export-api.ts [--api-base url]')
    return
  }

  const outDir = resolve(process.cwd(), PUBLIC_API_DIR)
  const apiBase = (values['api-base'] as string) || DEFAULT_API_BASE
  // sanity-check the api base is an absolute http(s) URL
  try {
    const parsed = new URL(apiBase)
    if (
      parsed.protocol !== 'https:' &&
      parsed.hostname !== 'localhost' &&
      parsed.hostname !== '127.0.0.1'
    ) {
      fail(`--api-base must be an https URL (got: ${apiBase})`)
    }
  } catch {
    fail(`--api-base must be a valid URL (got: ${apiBase})`)
  }

  console.log(`Reading index from ${INDEX_DIR}/ …`)
  const plugins = readIndexPlugins()
  console.log(`  ${plugins.length} plugins`)

  console.log('Building API files …')
  const { files, stats } = buildFiles(plugins, apiBase)

  rmSync(outDir, { recursive: true, force: true })
  for (const file of files) {
    const abs = join(outDir, file.relPath)
    mkdirSync(dirname(abs), { recursive: true })
    writeFileSync(abs, file.content)
  }

  console.log('Verifying output …')
  verify(outDir, plugins, stats)

  console.log(
    [
      `Done: ${stats.fileCount} files → ${relative(process.cwd(), outDir) || '.'}`,
      `  plugins=${stats.plugins} with_downloads=${stats.pluginsWithDownloads} versions=${stats.versions} files=${stats.releaseFiles} hashed=${stats.filesWithHashes}`,
      `  by_loader=${JSON.stringify(stats.byLoader)}`,
      `  search.json=${(stats.searchBytes / 1024).toFixed(0)} KiB, api_base=${apiBase}`,
    ]
      .filter(Boolean)
      .join('\n'),
  )
}

// Only run as an entry point (bun run scripts/export-api.ts), not on import
// (bun test imports this module for the pure helpers above).
if (import.meta.main) {
  try {
    main()
  } catch (err) {
    if (err instanceof ExportError) {
      console.error(`export-api: ${err.message}`)
    } else {
      console.error('export-api: unexpected failure', err)
    }
    process.exit(1)
  }
}
