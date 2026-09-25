// Shape contract shared with the exporter (single source of truth; the
// type-only import is erased at bundle time).
import type ApiV2 from '../src/types/api-v2'

/**
 * NukkitHub Public API v2 — dynamic routes (runtime-agnostic core)
 *
 * Routing architecture: plugins.nukkit-mot.com is
 * served through Tencent Cloud EdgeOne; the API lives under its /api
 * subtree with the origin on GitHub Pages. An edge function is bound to the
 * /api/v2 subtree with the trigger rule:
 *
 *   URL Path starts with /api/v2/  AND  URL Path does not contain ".json"
 *
 * so:
 *
 *   GET /api/v2/**    (suffixless) → edge function (this module)
 *   /api/v2/**.json   (static)     → never enters the function; EO rewrites
 *                                    the path (strips /api) and pulls it from
 *                                    the GitHub Pages export repo
 *   /api/** (other)                → never enters the function; EO rewrite only
 *
 * This is what makes the API addressable the Modrinth way: Modrinth clients
 * that accept a custom base URL point at `https://plugins.nukkit-mot.com/api`
 * and request `/v2/...` without suffixes. The function runs dynamic search
 * natively and serves every other suffixless route by proxying the sibling
 * static .json file from the SAME origin — one source of truth, so deploying
 * only means pushing the export to GitHub Pages; the edge function is
 * deploy-once code (see api-worker/eo.ts for the EdgeOne entry point and
 * `bun run build:eo` for the single-file bundle).
 *
 * Modrinth-compatible routes under /api/v2 (suffixless):
 *   GET /search                    dynamic search: query / facets / index /
 *                                  offset / limit (Modrinth parameter syntax)
 *   GET /projects?ids=["a/b",…]    batch project lookup, ≤ 20 ids (slugs ok)
 *   GET /project/{owner}/{name}    project detail; also accepts a
 *                                  single-segment slug, and %2F-encoded ids
 *   GET /project/{id}/version      version list; optional ?loaders= and
 *                                  ?game_versions= JSON-array filters
 *   GET /project/{id}/version/{n}  single version (version numbers are only
 *                                  unique within a project)
 *   GET /project/{id}/latest       NukkitHub extension: latest installable
 *                                  version (first non-prerelease)
 *   GET /version/{version_id}      single version by globally-unique
 *                                  "owner/name@version_number" id
 *   GET /versions?ids=["owner/name@v",…]  batch version lookup, ≤ 20 ids
 *   GET /version_file/{hash}       version owning the file with this hash
 *                                  (lowercase hex, sha1/sha256/sha512
 *                                  lengths; today the export carries sha256
 *                                  only — other algorithms 404)
 *   GET /tag/{name}                loader / category / game_version tags
 *   GET /meta                      index metadata
 *
 * Modrinth query parameters on /search:
 *   query    substring match over title + description (max 256 chars)
 *   facets   JSON array of arrays: outer AND, inner OR, e.g.
 *            [["loaders:nkmot","loaders:pnx"],["categories:economy"]]
 *            supported keys: loaders, categories, versions (version ids),
 *            game_versions, project_type, license, author, title
 *   index    relevance (default) | downloads | follows | newest | updated | stars
 *            (downloads/follows are approximated by stars until telemetry
 *            lands — see docs/API.md "Data caveats")
 *   offset   >= 0 (default 0)
 *   limit    1..100 (default 20)
 */

/** Minimal fetch signature for the injectable dataset fetcher (tests inject
 * a mock; production uses the runtime's global fetch). */
export type DatasetFetch = (input: string | URL | Request) => Promise<Response>

export type SearchHit = ApiV2.SearchHit
type SearchResponse = ApiV2.SearchResponse

// ---------------------------------------------------------------------------
// Limits (DoS guardrails for query-string input)
// ---------------------------------------------------------------------------

const MAX_QUERY_CHARS = 256
const MAX_FACET_GROUPS = 16
const MAX_FACET_TERMS_PER_GROUP = 16
const MAX_LIMIT = 100
const DEFAULT_LIMIT = 20
const MAX_PARAM_TERMS = 32
const MAX_BATCH_IDS = 20
/** The API is mounted under the /api subtree of the shared domain. */
const API_PREFIX = '/api'
const VERSION_PREFIX = `${API_PREFIX}/v2`
const DATASET_PATH = `${VERSION_PREFIX}/search.json`
/**
 * Marks subrequests this function makes itself. The trigger rule excludes
 * .json paths, so proxy/dataset subrequests cannot re-enter the function;
 * this header breaks any misconfiguration loudly instead of recursing.
 */
const INTERNAL_HEADER = 'x-nh-internal'

/** Tag files that exist in the static export (see scripts/export-api.ts) */
const TAG_NAMES = new Set(['loader', 'category', 'game_version'])

/** URL/filename-safe characters of GitHub-derived owner/name segments */
const SEGMENT_RE = /^[A-Za-z0-9_.-]+$/
/** Version slugs as produced by the exporter's toVersionSlug() */
const VERSION_RE = /^[A-Za-z0-9][A-Za-z0-9._+-]*$/
/** File-hash lookups: lowercase hex, sha1 / sha256 / sha512 lengths */
const FILE_HASH_RE = /^(?:[0-9a-f]{40}|[0-9a-f]{64}|[0-9a-f]{128})$/

class BadRequestError extends Error {}

// ---------------------------------------------------------------------------
// Pure query logic (exported for unit tests)
// ---------------------------------------------------------------------------

/** One parsed term, e.g. { key: 'loaders', value: 'nkmot' } */
export interface FacetTerm {
  key: string
  value: string
}

/**
 * Parse Modrinth-style facets: `[["a:b","c:d"],["e:f"]]`.
 * Outer array AND, inner arrays OR. Values may be quoted. Throws
 * BadRequestError on malformed input or excessive size.
 */
export function parseFacets(raw: string | null): FacetTerm[][] {
  if (raw === null || raw.trim() === '') return []
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    throw new BadRequestError('facets must be valid JSON')
  }
  if (!Array.isArray(parsed)) {
    throw new BadRequestError('facets must be a JSON array of arrays')
  }
  if (parsed.length > MAX_FACET_GROUPS) {
    throw new BadRequestError(`facets: at most ${MAX_FACET_GROUPS} groups`)
  }
  const groups: FacetTerm[][] = []
  for (const group of parsed) {
    if (!Array.isArray(group)) {
      throw new BadRequestError('facets must be a JSON array of arrays')
    }
    if (group.length > MAX_FACET_TERMS_PER_GROUP) {
      throw new BadRequestError(
        `facets: at most ${MAX_FACET_TERMS_PER_GROUP} terms per group`,
      )
    }
    const terms: FacetTerm[] = []
    for (const term of group) {
      if (typeof term !== 'string') {
        throw new BadRequestError('facet terms must be strings')
      }
      const unquoted = term.replace(/^"(.*)"$/, '$1')
      const separator = unquoted.indexOf(':')
      if (separator <= 0 || separator === unquoted.length - 1) {
        throw new BadRequestError(
          `invalid facet term "${term}" (expected "key:value")`,
        )
      }
      terms.push({
        key: unquoted.slice(0, separator).toLowerCase(),
        value: unquoted
          .slice(separator + 1)
          .replace(/^"(.*)"$/, '$1')
          .toLowerCase(),
      })
    }
    if (terms.length > 0) groups.push(terms)
  }
  return groups
}

const FACET_KEYS = new Set([
  'loaders',
  'categories',
  'versions',
  'game_versions',
  'project_type',
  'license',
  'author',
  'title',
])

function matchFacetTerm(hit: SearchHit, term: FacetTerm): boolean {
  switch (term.key) {
    case 'loaders':
      return hit.loaders.some((v) => v.toLowerCase() === term.value)
    case 'categories':
      return hit.categories.some((v) => v.toLowerCase() === term.value)
    case 'versions':
      return hit.versions.some((v) => v.toLowerCase() === term.value)
    case 'game_versions':
      return hit.game_versions.some((v) => v.toLowerCase() === term.value)
    case 'project_type':
      return hit.project_type.toLowerCase() === term.value
    case 'license':
      return hit.license.id.toLowerCase() === term.value
    case 'author':
      return hit.author.toLowerCase() === term.value
    case 'title':
      return hit.title.toLowerCase() === term.value
    default:
      // unknown keys match nothing rather than erroring (forward compatible:
      // consumers can send richer facets against future versions)
      return false
  }
}

/** Filter hits: query substring (title+description, case-insensitive) AND all facet groups (inner OR). */
export function filterHits(
  hits: SearchHit[],
  query: string,
  facetGroups: FacetTerm[][],
): SearchHit[] {
  const q = query.trim().toLowerCase()
  return hits.filter((hit) => {
    if (
      q &&
      !hit.title.toLowerCase().includes(q) &&
      !hit.description.toLowerCase().includes(q)
    ) {
      return false
    }
    for (const group of facetGroups) {
      if (
        !group.some(
          (term) => FACET_KEYS.has(term.key) && matchFacetTerm(hit, term),
        )
      ) {
        return false
      }
    }
    return true
  })
}

export type SortIndex =
  | 'relevance'
  | 'downloads'
  | 'follows'
  | 'newest'
  | 'updated'
  | 'stars'

export function parseIndex(raw: string | null): SortIndex {
  switch ((raw ?? '').toLowerCase()) {
    case 'downloads':
      return 'downloads'
    case 'follows':
      return 'follows'
    case 'newest':
      return 'newest'
    case 'updated':
      return 'updated'
    case 'stars':
      return 'stars'
    default:
      return 'relevance'
  }
}

/** relevance with a query: title-prefix > title-contains > description-contains. */
function relevanceScore(hit: SearchHit, query: string): number {
  const q = query.trim().toLowerCase()
  if (!q) return 0
  const title = hit.title.toLowerCase()
  if (title.startsWith(q)) return 3
  if (title.includes(q)) return 2
  return 1 // matched description in filterHits
}

export function sortHits(
  hits: SearchHit[],
  index: SortIndex,
  query: string,
): SearchHit[] {
  const sorted = [...hits]
  switch (index) {
    case 'newest':
      sorted.sort(
        (a, b) =>
          b.date_created.localeCompare(a.date_created) ||
          b.project_id.localeCompare(a.project_id),
      )
      break
    // downloads/follows approximate with stars until download telemetry exists
    case 'stars':
    case 'downloads':
    case 'follows':
      sorted.sort(
        (a, b) => b.stars - a.stars || b.project_id.localeCompare(a.project_id),
      )
      break
    case 'updated':
      sorted.sort(
        (a, b) =>
          b.date_modified.localeCompare(a.date_modified) ||
          b.project_id.localeCompare(a.project_id),
      )
      break
    case 'relevance':
    default: {
      const byRecency = (a: SearchHit, b: SearchHit) =>
        b.date_modified.localeCompare(a.date_modified) ||
        b.project_id.localeCompare(a.project_id)
      if (query.trim()) {
        sorted.sort(
          (a, b) =>
            relevanceScore(b, query) - relevanceScore(a, query) ||
            byRecency(a, b),
        )
      } else {
        sorted.sort(byRecency)
      }
      break
    }
  }
  return sorted
}

export function clampInt(
  raw: string | null,
  fallback: number,
  min: number,
  max: number,
): number {
  if (raw === null || raw.trim() === '') return fallback
  const parsed = Number.parseInt(raw, 10)
  if (!Number.isFinite(parsed)) return fallback
  return Math.min(max, Math.max(min, parsed))
}

// ---------------------------------------------------------------------------
// Pure routing helpers (exported for unit tests)
// ---------------------------------------------------------------------------

function decodeSegment(segment: string): string {
  try {
    return decodeURIComponent(segment)
  } catch {
    return ''
  }
}

/**
 * Normalize a still-percent-encoded project reference (raw path segments
 * joined by "/") into one or two decoded segments — either ["owner","name"]
 * or ["slug"]. Accepts SDKs that encode the whole "owner/name" as one
 * parameter ("owner%2Fname"). Returns null for anything else, including
 * "." / ".." traversal attempts.
 */
export function normalizeRefString(raw: string): string[] | null {
  const decoded = decodeSegment(raw)
  if (!decoded) return null
  const parts = decoded.split('/')
  if (parts.length < 1 || parts.length > 2) return null
  for (const part of parts) {
    if (!SEGMENT_RE.test(part) || part === '.' || part === '..') return null
  }
  return parts
}

export interface VersionRef {
  projectId: string
  versionSlug: string
}

/**
 * Parse a globally-unique version id — "owner/name@version_number" — from a
 * still-percent-encoded raw path (segments joined by "/"). "@" separates
 * unambiguously: it can appear in neither project id segments nor version
 * slugs. Bare version numbers are rejected (they are only unique within a
 * project); so are single-segment project refs, "." / ".." traversal, and
 * slugs with path-unsafe characters. Returns null for anything else.
 */
export function parseVersionId(raw: string): VersionRef | null {
  const decoded = decodeSegment(raw)
  if (!decoded) return null
  const at = decoded.indexOf('@')
  if (at <= 0) return null
  const parts = decoded.slice(0, at).split('/')
  if (parts.length !== 2) return null
  for (const part of parts) {
    if (!SEGMENT_RE.test(part) || part === '.' || part === '..') return null
  }
  const versionSlug = decoded.slice(at + 1)
  if (!VERSION_RE.test(versionSlug)) return null
  return { projectId: `${parts[0]}/${parts[1]}`, versionSlug }
}

export type ProjectSubRoute =
  | { kind: 'detail' }
  | { kind: 'versions' }
  | { kind: 'version'; versionSlug: string }
  | { kind: 'latest' }

export interface ProjectRoute {
  refRaw: string
  sub: ProjectSubRoute
}

/**
 * Split raw path segments after "/api/v2/project/" into the project
 * reference and its sub-resource. The first "version" / "latest" segment at
 * index ≥ 1 is the marker (a single-segment id can never be the marker
 * itself); "version" consumes one optional trailing version slug.
 */
export function parseProjectRoute(segments: string[]): ProjectRoute | null {
  for (let i = 1; i < segments.length; i++) {
    const seg = segments[i]
    if (seg === 'latest') {
      if (segments.length !== i + 1) return null
      return { refRaw: segments.slice(0, i).join('/'), sub: { kind: 'latest' } }
    }
    if (seg === 'version') {
      const refRaw = segments.slice(0, i).join('/')
      if (segments.length === i + 1) {
        return { refRaw, sub: { kind: 'versions' } }
      }
      if (segments.length === i + 2) {
        return {
          refRaw,
          sub: { kind: 'version', versionSlug: segments[i + 1] },
        }
      }
      return null
    }
  }
  return { refRaw: segments.join('/'), sub: { kind: 'detail' } }
}

/**
 * slug (lowercase) → project_id for slug lookups. Ambiguous slugs map to ''
 * and never resolve — like Modrinth's single-segment addressing, a slug must
 * identify exactly one project.
 */
export function buildSlugIndex(hits: SearchHit[]): Map<string, string> {
  const index = new Map<string, string>()
  for (const hit of hits) {
    const key = hit.slug.toLowerCase()
    const existing = index.get(key)
    if (existing === undefined) index.set(key, hit.project_id)
    else if (existing !== hit.project_id) index.set(key, '')
  }
  return index
}

/**
 * Parse a Modrinth-style JSON string-array query parameter (e.g. `loaders`
 * on the version list). Returns null when absent/empty; throws
 * BadRequestError on malformed input.
 */
export function parseStringArrayParam(
  raw: string | null,
  name: string,
): string[] | null {
  if (raw === null || raw.trim() === '') return null
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    throw new BadRequestError(`${name} must be a JSON array of strings`)
  }
  if (
    !Array.isArray(parsed) ||
    parsed.length > MAX_PARAM_TERMS ||
    !parsed.every((v) => typeof v === 'string')
  ) {
    throw new BadRequestError(`${name} must be a JSON array of strings`)
  }
  return (parsed as string[]).map((v) => v.toLowerCase())
}

// ---------------------------------------------------------------------------
// HTTP layer
// ---------------------------------------------------------------------------

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'access-control-allow-origin': '*',
      'cache-control': 'public, max-age=300, s-maxage=600',
      'x-content-type-options': 'nosniff',
    },
  })
}

function notFound(): Response {
  return jsonResponse({ error: 'Not Found' }, 404)
}

function internalFetchInit(): RequestInit {
  return { method: 'GET', headers: { [INTERNAL_HEADER]: '1' } }
}

/**
 * Load the dataset from this origin's /v2/search.json (served by GitHub
 * Pages through normal edge proxying). Uses the runtime Cache API when
 * available so bursts of dynamic queries do not re-fetch the asset each
 * time (absent on some runtimes — the call is simply skipped).
 *
 * Resilience: cold cache misses must pull the asset cross-region from the
 * static origin, which fails intermittently; each failed attempt (throw or
 * non-ok status) is retried before giving up, and a broken cache read or
 * write degrades to a plain fetch instead of failing the request.
 */
const DATASET_ATTEMPTS = 3

async function fetchDatasetResilient(
  datasetUrl: URL,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  const doFetch: DatasetFetch = dataFetch ?? fetch
  let lastError: unknown = new Error('dataset fetch failed')
  for (let attempt = 0; attempt < DATASET_ATTEMPTS; attempt++) {
    try {
      const res = await doFetch(
        new Request(datasetUrl.toString(), internalFetchInit()),
      )
      if (res.ok) return res
      lastError = new Error(`dataset fetch failed with status ${res.status}`)
    } catch (err) {
      lastError = err
    }
  }
  throw lastError
}

export async function fetchDataset(
  url: URL,
  dataFetch?: DatasetFetch,
): Promise<SearchResponse> {
  const datasetUrl = new URL(DATASET_PATH, url.origin)
  const cache = (globalThis as unknown as { caches?: { default: Cache } })
    .caches?.default
  if (cache) {
    try {
      const cached = await cache.match(datasetUrl.toString())
      if (cached) return (await cached.json()) as SearchResponse
    } catch {
      // unreadable cache entry — fall through to a fresh fetch
    }
  }
  const res = await fetchDatasetResilient(datasetUrl, dataFetch)
  const body = (await res.json()) as SearchResponse
  if (cache) {
    try {
      // re-serialize instead of cloning: a put() failure mid-stream must not
      // consume the body we are about to return
      await cache.put(
        datasetUrl.toString(),
        new Response(JSON.stringify(body), {
          headers: {
            'content-type': 'application/json; charset=utf-8',
            'cache-control': 'public, max-age=600',
          },
        }),
      )
    } catch {
      // unwritable cache — the response is still served
    }
  }
  return body
}

export async function handleSearch(
  url: URL,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  const query = (url.searchParams.get('query') ?? '').slice(0, MAX_QUERY_CHARS)
  let facetGroups: FacetTerm[][]
  try {
    facetGroups = parseFacets(url.searchParams.get('facets'))
  } catch (err) {
    if (err instanceof BadRequestError) {
      return jsonResponse({ error: 'Bad Request', message: err.message }, 400)
    }
    throw err
  }
  const index = parseIndex(url.searchParams.get('index'))
  const offset = clampInt(url.searchParams.get('offset'), 0, 0, 1_000_000)
  const limit = clampInt(
    url.searchParams.get('limit'),
    DEFAULT_LIMIT,
    1,
    MAX_LIMIT,
  )

  let dataset: SearchResponse
  try {
    dataset = await fetchDataset(url, dataFetch)
  } catch {
    return jsonResponse(
      { error: 'Service Unavailable', message: 'search dataset unavailable' },
      502,
    )
  }
  const filtered = filterHits(dataset.hits, query, facetGroups)
  const sorted = sortHits(filtered, index, query)
  const page = sorted.slice(offset, offset + limit)

  return jsonResponse({
    offset,
    limit,
    total_hits: sorted.length,
    hits: page,
  })
}

/**
 * Serve a suffixless route by streaming the sibling static .json file from
 * this origin (the trigger rule excludes .json paths, so this subrequest is
 * answered by the static origin, never by the function itself).
 */
async function proxyStaticJson(
  url: URL,
  staticPath: string,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  const doFetch: DatasetFetch = dataFetch ?? fetch
  try {
    const res = await doFetch(
      new Request(
        new URL(staticPath, url.origin).toString(),
        internalFetchInit(),
      ),
    )
    if (res.status === 404) return notFound()
    if (!res.ok) {
      return jsonResponse({ error: 'Bad Gateway' }, 502)
    }
    return new Response(res.body, {
      status: 200,
      headers: {
        'content-type': 'application/json; charset=utf-8',
        'access-control-allow-origin': '*',
        'cache-control': 'public, max-age=300, s-maxage=600',
        'x-content-type-options': 'nosniff',
      },
    })
  } catch {
    return jsonResponse({ error: 'Bad Gateway' }, 502)
  }
}

/**
 * Resolve a project reference to the canonical "owner/name" id. Two-segment
 * refs pass through; a single segment is treated as a slug and must resolve
 * to exactly one indexed plugin (see buildSlugIndex). Returns null when the
 * reference cannot be resolved (→ 404).
 */
async function resolveProjectId(
  ref: string[],
  url: URL,
  dataFetch?: DatasetFetch,
): Promise<string | null> {
  if (ref.length === 2) return `${ref[0]}/${ref[1]}`
  try {
    const dataset = await fetchDataset(url, dataFetch)
    return buildSlugIndex(dataset.hits).get(ref[0].toLowerCase()) || null
  } catch {
    return null
  }
}

/**
 * Parse a Modrinth-style ?ids=["…"] batch parameter. Returns the validated
 * string array, or a 400 Response for missing/malformed/oversized input.
 */
function parseBatchIdsParam(url: URL): string[] | Response {
  const idsRaw = url.searchParams.get('ids')
  if (idsRaw === null || idsRaw.trim() === '') {
    return jsonResponse(
      { error: 'Bad Request', message: 'missing required "ids" parameter' },
      400,
    )
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(idsRaw)
  } catch {
    return jsonResponse(
      { error: 'Bad Request', message: 'ids must be a JSON array of strings' },
      400,
    )
  }
  if (!Array.isArray(parsed) || !parsed.every((v) => typeof v === 'string')) {
    return jsonResponse(
      { error: 'Bad Request', message: 'ids must be a JSON array of strings' },
      400,
    )
  }
  const ids = parsed as string[]
  if (ids.length > MAX_BATCH_IDS) {
    return jsonResponse(
      { error: 'Bad Request', message: `ids: at most ${MAX_BATCH_IDS}` },
      400,
    )
  }
  return ids
}

/**
 * Fetch several same-origin static .json files in parallel and return the
 * decoded bodies; 404s and failures are skipped (Modrinth batch semantics —
 * unresolvable ids simply drop out of the result).
 */
async function fetchStaticBodies(
  url: URL,
  paths: string[],
  dataFetch?: DatasetFetch,
): Promise<unknown[]> {
  const doFetch: DatasetFetch = dataFetch ?? fetch
  const bodies = await Promise.all(
    paths.map(async (path) => {
      try {
        const res = await doFetch(
          new Request(
            new URL(path, url.origin).toString(),
            internalFetchInit(),
          ),
        )
        return res.ok ? ((await res.json()) as unknown) : null
      } catch {
        return null
      }
    }),
  )
  return bodies.filter((body) => body !== null)
}

/**
 * GET /projects?ids=["owner/name", "slug", …] — Modrinth-style batch lookup.
 * Unresolvable ids are skipped (Modrinth semantics); returns a bare array.
 */
async function handleProjects(
  url: URL,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  const idsOrError = parseBatchIdsParam(url)
  if (idsOrError instanceof Response) return idsOrError

  const refs = idsOrError
    .map((id) => normalizeRefString(id))
    .filter((ref): ref is string[] => ref !== null)
  const slugs = refs.filter((ref) => ref.length === 1)
  const projectIds = refs
    .filter((ref) => ref.length === 2)
    .map((ref) => ref.join('/'))
  if (slugs.length > 0) {
    try {
      const dataset = await fetchDataset(url, dataFetch)
      const slugIndex = buildSlugIndex(dataset.hits)
      for (const slug of slugs) {
        const id = slugIndex.get(slug[0].toLowerCase())
        if (id) projectIds.push(id)
      }
    } catch {
      return jsonResponse(
        { error: 'Service Unavailable', message: 'search dataset unavailable' },
        502,
      )
    }
  }
  if (projectIds.length === 0) return jsonResponse([])
  const bodies = await fetchStaticBodies(
    url,
    projectIds.map((id) => {
      const [owner, name] = id.split('/')
      return `${VERSION_PREFIX}/project/${owner}/${name}.json`
    }),
    dataFetch,
  )
  return jsonResponse(bodies)
}

/**
 * GET /versions?ids=["owner/name@version",…] — Modrinth-style batch version
 * lookup by globally-unique version id (bare version numbers are only unique
 * within a project and cannot resolve). Unresolvable ids are skipped;
 * returns a bare array.
 */
async function handleVersionsBatch(
  url: URL,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  const idsOrError = parseBatchIdsParam(url)
  if (idsOrError instanceof Response) return idsOrError

  const refs = idsOrError
    .map((id) => parseVersionId(id))
    .filter((ref): ref is VersionRef => ref !== null)
  if (refs.length === 0) return jsonResponse([])
  const bodies = await fetchStaticBodies(
    url,
    refs.map(({ projectId, versionSlug }) => {
      const [owner, name] = projectId.split('/')
      return `${VERSION_PREFIX}/project/${owner}/${name}/version/${versionSlug}.json`
    }),
    dataFetch,
  )
  return jsonResponse(bodies)
}

/**
 * GET /project/{id}/version — bare version array, with Modrinth's optional
 * ?loaders= and ?game_versions= JSON-array filters applied server-side.
 */
async function handleVersionList(
  url: URL,
  owner: string,
  name: string,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  let loaders: string[] | null = null
  let gameVersions: string[] | null = null
  try {
    loaders = parseStringArrayParam(url.searchParams.get('loaders'), 'loaders')
    gameVersions = parseStringArrayParam(
      url.searchParams.get('game_versions'),
      'game_versions',
    )
  } catch (err) {
    if (err instanceof BadRequestError) {
      return jsonResponse({ error: 'Bad Request', message: err.message }, 400)
    }
    throw err
  }
  const staticPath = `${VERSION_PREFIX}/project/${owner}/${name}/version.json`
  if (loaders === null && gameVersions === null) {
    return proxyStaticJson(url, staticPath, dataFetch)
  }
  const doFetch: DatasetFetch = dataFetch ?? fetch
  try {
    const res = await doFetch(
      new Request(
        new URL(staticPath, url.origin).toString(),
        internalFetchInit(),
      ),
    )
    if (res.status === 404) return notFound()
    if (!res.ok) return jsonResponse({ error: 'Bad Gateway' }, 502)
    const versions = (await res.json()) as ApiV2.Version[]
    const filtered = versions.filter(
      (v) =>
        (loaders === null ||
          v.loaders.some((l) => loaders.includes(l.toLowerCase()))) &&
        (gameVersions === null ||
          v.game_versions.some((g) => gameVersions.includes(g.toLowerCase()))),
    )
    return jsonResponse(filtered)
  } catch {
    return jsonResponse({ error: 'Bad Gateway' }, 502)
  }
}

async function handleProjectRoutes(
  segments: string[],
  url: URL,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  const route = parseProjectRoute(segments)
  if (route === null) return notFound()
  const ref = normalizeRefString(route.refRaw)
  if (ref === null) return notFound()
  const projectId = await resolveProjectId(ref, url, dataFetch)
  if (projectId === null) return notFound()
  const [owner, name] = projectId.split('/')
  const base = `${VERSION_PREFIX}/project/${owner}/${name}`
  switch (route.sub.kind) {
    case 'detail':
      return proxyStaticJson(url, `${base}.json`, dataFetch)
    case 'versions':
      return handleVersionList(url, owner, name, dataFetch)
    case 'version': {
      const versionSlug = decodeSegment(route.sub.versionSlug)
      if (!versionSlug || !VERSION_RE.test(versionSlug)) return notFound()
      return proxyStaticJson(
        url,
        `${base}/version/${versionSlug}.json`,
        dataFetch,
      )
    }
    case 'latest':
      return proxyStaticJson(url, `${base}/latest.json`, dataFetch)
  }
}

/** Path segments after "/api/v2/", or null when the path is outside /api/v2. */
function routeSegments(pathname: string): string[] | null {
  if (pathname === VERSION_PREFIX) return []
  if (!pathname.startsWith(`${VERSION_PREFIX}/`)) return null
  return pathname.slice(VERSION_PREFIX.length + 1).split('/')
}

async function route(
  segments: string[],
  url: URL,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  const [head, ...tail] = segments
  switch (head) {
    case 'search': {
      if (tail.length === 0) return handleSearch(url, dataFetch)
      // /search/{loader} — suffixless form of the pre-filtered static file
      if (tail.length === 1) {
        const loader = decodeSegment(tail[0])
        if (!loader || !SEGMENT_RE.test(loader) || loader.includes('.'))
          return notFound()
        return proxyStaticJson(
          url,
          `${VERSION_PREFIX}/search/${loader}.json`,
          dataFetch,
        )
      }
      return notFound()
    }
    case 'projects':
      if (tail.length === 0) return handleProjects(url, dataFetch)
      return notFound()
    case 'project':
      if (tail.length === 0) return notFound()
      return handleProjectRoutes(tail, url, dataFetch)
    case 'version': {
      if (tail.length === 0) return notFound()
      // one segment when the id is %2F-encoded, two when a proxy already
      // decoded the slash — joining first covers both forms
      const ref = parseVersionId(tail.join('/'))
      if (ref === null) return notFound()
      const [owner, name] = ref.projectId.split('/')
      return proxyStaticJson(
        url,
        `${VERSION_PREFIX}/project/${owner}/${name}/version/${ref.versionSlug}.json`,
        dataFetch,
      )
    }
    case 'versions':
      if (tail.length === 0) return handleVersionsBatch(url, dataFetch)
      return notFound()
    case 'version_file': {
      if (tail.length !== 1) return notFound()
      const hash = decodeSegment(tail[0]).toLowerCase()
      if (!hash || !FILE_HASH_RE.test(hash)) return notFound()
      return proxyStaticJson(
        url,
        `${VERSION_PREFIX}/version_file/${hash}.json`,
        dataFetch,
      )
    }
    case 'tag':
      if (tail.length === 1 && TAG_NAMES.has(tail[0])) {
        return proxyStaticJson(
          url,
          `${VERSION_PREFIX}/tag/${tail[0]}.json`,
          dataFetch,
        )
      }
      return notFound()
    case 'meta':
      if (tail.length === 0) {
        return proxyStaticJson(url, `${VERSION_PREFIX}/meta.json`, dataFetch)
      }
      return notFound()
    default:
      return notFound()
  }
}

/**
 * Full request handler shared by runtime entry points (EdgeOne edge
 * function in production, unit tests elsewhere). `dataFetch` is injectable
 * for tests; production passes nothing and the global fetch is used.
 */
export async function handleRequest(
  request: Request,
  dataFetch?: DatasetFetch,
): Promise<Response> {
  // Loop breaker: a request carrying our own internal marker must never be
  // processed again (see INTERNAL_HEADER). Fails loudly if the trigger rule
  // is ever misconfigured to match the function's own subrequests.
  if (request.headers.get(INTERNAL_HEADER)) {
    return jsonResponse(
      {
        error: 'Loop Detected',
        message: 'edge function trigger appears to match its own subrequests',
      },
      508,
    )
  }
  const url = new URL(request.url)
  const segments = routeSegments(url.pathname)
  // Anything outside /api/v2 reaching us means misconfiguration — do not
  // proxy anywhere (that could recurse), just refuse.
  if (segments === null) {
    return notFound()
  }
  // Static .json paths must never reach the function (the trigger rule
  // excludes them); proxying them would recurse into ourselves.
  if (url.pathname.includes('.json')) {
    return jsonResponse(
      {
        error: 'Loop Detected',
        message: 'edge function trigger appears to match static .json paths',
      },
      508,
    )
  }
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    return jsonResponse({ error: 'Method Not Allowed' }, 405)
  }
  return route(segments, url, dataFetch)
}
