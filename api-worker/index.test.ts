/**
 * Unit + integration tests for api-worker/index.ts.
 * Run with: bun test api-worker/
 *
 * In production the function owns the suffixless /api/v2/** routes only
 * (static .json files never enter it — the EdgeOne trigger rule excludes
 * them), so integration tests inject a mock DATA_FETCH that stands in for
 * the same-origin static origin: /api/v2/search.json plus any static file
 * the test declares.
 */

/// <reference types="bun-types" />
import { describe, expect, test } from 'bun:test'

import {
  buildSlugIndex,
  clampInt,
  type DatasetFetch,
  type FacetTerm,
  filterHits,
  handleRequest,
  normalizeRefString,
  parseFacets,
  parseIndex,
  parseProjectRoute,
  parseStringArrayParam,
  parseVersionId,
  type SearchHit,
  sortHits
} from './index'

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

function makeHit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    project_id: 'a/one',
    project_type: 'plugin',
    slug: 'one',
    author: 'Alice',
    title: 'One',
    description: 'first plugin',
    categories: ['economy'],
    loaders: ['nkmot'],
    game_versions: ['1.0.0'],
    versions: ['a/one@v1'],
    downloads: 0,
    icon_url: '',
    date_created: '2024-01-01T00:00:00.000Z',
    date_modified: '2024-01-02T00:00:00.000Z',
    latest_version: 'v1',
    license: { id: 'MIT', name: 'MIT', url: '' },
    stars: 1,
    ...overrides,
  }
}

const HITS: SearchHit[] = [
  makeHit(),
  makeHit({
    project_id: 'b/two',
    slug: 'two',
    author: 'Bob',
    title: 'Two Economy',
    description: 'economy addon',
    categories: ['economy', 'utility'],
    loaders: ['pnx', 'nkmot'],
    versions: ['b/two@v2'],
    date_created: '2024-03-01T00:00:00.000Z',
    date_modified: '2024-03-02T00:00:00.000Z',
    stars: 50,
  }),
  makeHit({
    project_id: 'c/three',
    slug: 'three',
    author: 'Carol',
    title: 'Three',
    description: 'combat plugin',
    categories: ['adventure'],
    loaders: ['lumi'],
    versions: ['c/three@v3'],
    date_created: '2024-02-01T00:00:00.000Z',
    date_modified: '2024-02-02T00:00:00.000Z',
    stars: 10,
  }),
]

// ---------------------------------------------------------------------------
// parseFacets
// ---------------------------------------------------------------------------

describe('parseFacets', () => {
  test('empty input → no groups', () => {
    expect(parseFacets(null)).toEqual([])
    expect(parseFacets('')).toEqual([])
    expect(parseFacets('  ')).toEqual([])
  })
  test('parses AND/OR groups and strips quotes', () => {
    expect(
      parseFacets('[["loaders:nkmot","loaders:pnx"],["categories:economy"]]'),
    ).toEqual<FacetTerm[][]>([
      [
        { key: 'loaders', value: 'nkmot' },
        { key: 'loaders', value: 'pnx' },
      ],
      [{ key: 'categories', value: 'economy' }],
    ])
    expect(parseFacets('[["title:\\"Two Economy\\""]]')).toEqual<FacetTerm[][]>(
      [[{ key: 'title', value: 'two economy' }]],
    )
  })
  test('rejects malformed JSON', () => {
    expect(() => parseFacets('not json')).toThrow()
    expect(() => parseFacets('["loaders:nkmot"]')).toThrow() // not array-of-arrays
    expect(() => parseFacets('[[42]]')).toThrow() // non-string term
    expect(() => parseFacets('[["novalue"]]')).toThrow() // missing ':'
    expect(() => parseFacets('[["":x]]')).toThrow() // empty key
  })
  test('rejects excessive size', () => {
    const big = JSON.stringify(
      Array.from({ length: 17 }, () => ['loaders:nkmot']),
    )
    expect(() => parseFacets(big)).toThrow()
    const wide = JSON.stringify([
      Array.from({ length: 17 }, (_, i) => `loaders:l${i}`),
    ])
    expect(() => parseFacets(wide)).toThrow()
  })
})

// ---------------------------------------------------------------------------
// filterHits
// ---------------------------------------------------------------------------

describe('filterHits', () => {
  test('query matches title or description case-insensitively', () => {
    // only b/two contains 'economy' in title ('Two Economy') or description
    expect(filterHits(HITS, 'economy', []).map((h) => h.project_id)).toEqual([
      'b/two',
    ])
    // c/three matches via description ('combat plugin')
    expect(filterHits(HITS, 'COMBAT', []).map((h) => h.project_id)).toEqual([
      'c/three',
    ])
  })
  test('facet groups AND between groups, OR within a group', () => {
    const groups = parseFacets('[["loaders:nkmot"],["categories:economy"]]')
    expect(filterHits(HITS, '', groups).map((h) => h.project_id)).toEqual([
      'a/one',
      'b/two',
    ])
    const orGroup = parseFacets('[["loaders:pnx","loaders:lumi"]]')
    expect(filterHits(HITS, '', orGroup).map((h) => h.project_id)).toEqual([
      'b/two',
      'c/three',
    ])
  })
  test('unknown facet keys match nothing (forward compatible)', () => {
    expect(filterHits(HITS, '', [[{ key: 'future_key', value: 'x' }]])).toEqual(
      [],
    )
  })
  test('versions facet matches version ids; game_versions stays separate', () => {
    const byId = parseFacets('[["versions:a/one@v1"]]')
    expect(filterHits(HITS, '', byId).map((h) => h.project_id)).toEqual([
      'a/one',
    ])
    const byGameVersion = parseFacets('[["game_versions:1.0.0"]]')
    expect(
      filterHits(HITS, '', byGameVersion).map((h) => h.project_id),
    ).toEqual(['a/one', 'b/two', 'c/three'])
  })
  test('combined query + facets', () => {
    const groups = parseFacets('[["categories:economy"]]')
    expect(filterHits(HITS, 'two', groups).map((h) => h.project_id)).toEqual([
      'b/two',
    ])
  })
})

// ---------------------------------------------------------------------------
// sortHits / clampInt / parseIndex
// ---------------------------------------------------------------------------

describe('sortHits', () => {
  test('updated sorts date_modified desc', () => {
    expect(sortHits(HITS, 'updated', '').map((h) => h.project_id)).toEqual([
      'b/two',
      'c/three',
      'a/one',
    ])
  })
  test('newest sorts date_created desc', () => {
    expect(sortHits(HITS, 'newest', '').map((h) => h.project_id)).toEqual([
      'b/two',
      'c/three',
      'a/one',
    ])
  })
  test('stars/downloads/follows sort by stars desc', () => {
    for (const index of ['stars', 'downloads', 'follows'] as const) {
      expect(sortHits(HITS, index, '').map((h) => h.project_id)).toEqual([
        'b/two',
        'c/three',
        'a/one',
      ])
    }
  })
  test('relevance with query: title match ranks above description match', () => {
    const ranked = sortHits(HITS, 'relevance', 'economy')
    expect(ranked[0].project_id).toBe('b/two')
  })
  test('relevance title prefix scores highest', () => {
    const prefixHit = makeHit({ project_id: 'd/prefix', title: 'EconomyX' })
    const ranked = sortHits([...HITS, prefixHit], 'relevance', 'economy')
    expect(ranked[0].project_id).toBe('d/prefix')
  })
})

describe('clampInt / parseIndex', () => {
  test('clamps to range with fallback', () => {
    expect(clampInt(null, 20, 1, 100)).toBe(20)
    expect(clampInt('5', 20, 1, 100)).toBe(5)
    expect(clampInt('999', 20, 1, 100)).toBe(100)
    expect(clampInt('-5', 20, 1, 100)).toBe(1)
    expect(clampInt('abc', 20, 1, 100)).toBe(20)
  })
  test('parseIndex maps known keys and defaults to relevance', () => {
    expect(parseIndex('newest')).toBe('newest')
    expect(parseIndex('STARS')).toBe('stars')
    expect(parseIndex('bogus')).toBe('relevance')
    expect(parseIndex(null)).toBe('relevance')
  })
})

// ---------------------------------------------------------------------------
// Routing pure helpers
// ---------------------------------------------------------------------------

describe('normalizeRefString', () => {
  test('two-segment and one-segment refs pass through decoded', () => {
    expect(normalizeRefString('a/b')).toEqual(['a', 'b'])
    expect(normalizeRefString('one')).toEqual(['one'])
    expect(normalizeRefString('a%2Fb')).toEqual(['a', 'b']) // SDK-encoded id
  })
  test('rejects traversal, empty, and 3+ segment refs', () => {
    expect(normalizeRefString('../etc')).toBeNull()
    expect(normalizeRefString('a/..')).toBeNull()
    expect(normalizeRefString('..%2F..')).toBeNull()
    expect(normalizeRefString('')).toBeNull()
    expect(normalizeRefString('a/b/c')).toBeNull()
    expect(normalizeRefString('a%2Fb%2Fc')).toBeNull()
    expect(normalizeRefString('bad%zz')).toBeNull() // invalid escape
    expect(normalizeRefString('has space')).toBeNull()
    expect(normalizeRefString('has/slash~tilde')).toBeNull()
  })
})

describe('parseProjectRoute', () => {
  test('detail, version list, single version, latest', () => {
    expect(parseProjectRoute(['a', 'b'])).toEqual({
      refRaw: 'a/b',
      sub: { kind: 'detail' },
    })
    expect(parseProjectRoute(['a', 'b', 'version'])).toEqual({
      refRaw: 'a/b',
      sub: { kind: 'versions' },
    })
    expect(parseProjectRoute(['a', 'b', 'version', 'v1.0'])).toEqual({
      refRaw: 'a/b',
      sub: { kind: 'version', versionSlug: 'v1.0' },
    })
    expect(parseProjectRoute(['a', 'b', 'latest'])).toEqual({
      refRaw: 'a/b',
      sub: { kind: 'latest' },
    })
    // %2F-encoded single-segment id with a sub-resource
    expect(parseProjectRoute(['a%2Fb', 'latest'])).toEqual({
      refRaw: 'a%2Fb',
      sub: { kind: 'latest' },
    })
  })
  test('malformed tails → null', () => {
    expect(parseProjectRoute(['a', 'latest', 'extra'])).toBeNull()
    expect(parseProjectRoute(['a', 'version', 'v1', 'extra'])).toBeNull()
  })
  test('marker search starts at index 1 (slug can never be the marker)', () => {
    // "version" as the first segment is the whole id, not a sub-resource
    expect(parseProjectRoute(['version'])).toEqual({
      refRaw: 'version',
      sub: { kind: 'detail' },
    })
  })
})

describe('parseVersionId', () => {
  test('parses owner/name@version ids in every accepted encoding', () => {
    const expected = { projectId: 'a/one', versionSlug: 'v2.2.3' }
    expect(parseVersionId('a%2Fone%40v2.2.3')).toEqual(expected) // fully encoded
    expect(parseVersionId('a%2Fone@v2.2.3')).toEqual(expected) // SDK-encoded id
    expect(parseVersionId('a/one@v2.2.3')).toEqual(expected) // slash decoded
  })
  test('rejects ids that cannot resolve globally', () => {
    expect(parseVersionId('v2.2.3')).toBeNull() // bare version number
    expect(parseVersionId('one@v2.2.3')).toBeNull() // single-segment project
    expect(parseVersionId('a/b/c@v1')).toBeNull() // three segments
    expect(parseVersionId('a/b@')).toBeNull() // empty version
    expect(parseVersionId('@v1')).toBeNull()
    expect(parseVersionId('a/b@v/1')).toBeNull() // unsafe version chars
    expect(parseVersionId('a/b@.hidden')).toBeNull()
    expect(parseVersionId('a%2F..%40v1')).toBeNull() // traversal
    expect(parseVersionId('bad%zz@v1')).toBeNull() // invalid escape
    expect(parseVersionId('')).toBeNull()
  })
})

describe('buildSlugIndex', () => {
  test('unique slugs resolve; ambiguous slugs never do', () => {
    const hits = [
      makeHit({ project_id: 'a/one', slug: 'one' }),
      makeHit({ project_id: 'b/one', slug: 'One' }), // same slug, other project
      makeHit({ project_id: 'c/two', slug: 'two' }),
    ]
    const index = buildSlugIndex(hits)
    expect(index.get('one')).toBe('')
    expect(index.get('two')).toBe('c/two')
  })
})

describe('parseStringArrayParam', () => {
  test('absent/empty → null; valid arrays lowercase', () => {
    expect(parseStringArrayParam(null, 'loaders')).toBeNull()
    expect(parseStringArrayParam('', 'loaders')).toBeNull()
    expect(parseStringArrayParam('  ', 'loaders')).toBeNull()
    expect(parseStringArrayParam('["NKmot"]', 'loaders')).toEqual(['nkmot'])
  })
  test('malformed → throw', () => {
    expect(() => parseStringArrayParam('notjson', 'loaders')).toThrow()
    expect(() => parseStringArrayParam('[1]', 'loaders')).toThrow()
    expect(() => parseStringArrayParam('["a"]', 'loaders')).not.toThrow()
  })
})

// ---------------------------------------------------------------------------
// Integration: Worker fetch handler with an injected DATA_FETCH
// ---------------------------------------------------------------------------

/**
 * Mock same-origin static origin: always serves /api/v2/search.json from the
 * given hits, plus any extra files the test declares (path → JSON string or
 * { status }).
 */
function makeOriginFetch(
  files: Record<string, string> = {},
  dataset: SearchHit[] = HITS,
): DatasetFetch & { requests: Request[] } {
  const requests: Request[] = []
  const datasetJson = JSON.stringify({
    offset: 0,
    limit: dataset.length,
    total_hits: dataset.length,
    hits: dataset,
  })
  const dataFetch = (input: string | URL | Request) => {
    const req = input instanceof Request ? input : new Request(String(input))
    requests.push(req)
    const path = new URL(req.url).pathname
    const body = path === '/api/v2/search.json' ? datasetJson : files[path]
    if (body === undefined) {
      return Promise.resolve(new Response('not found', { status: 404 }))
    }
    return Promise.resolve(
      new Response(body, {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    )
  }
  const tracked = dataFetch as DatasetFetch & { requests: Request[] }
  tracked.requests = requests
  return tracked
}

function call(
  pathAndQuery: string,
  dataFetch: DatasetFetch = makeOriginFetch(),
): Promise<Response> {
  return handleRequest(
    new Request(`https://plugins.nukkit-mot.com${pathAndQuery}`),
    dataFetch,
  )
}

describe('worker fetch handler: dynamic search', () => {
  test('filters, paginates, CORS + cache headers', async () => {
    const res = await call(
      '/api/v2/search?query=economy&facets=%5B%5B%22categories%3Aeconomy%22%5D%5D&limit=1&offset=0',
    )
    expect(res.status).toBe(200)
    expect(res.headers.get('access-control-allow-origin')).toBe('*')
    expect(res.headers.get('content-type')).toContain('application/json')
    const body = (await res.json()) as {
      total_hits: number
      limit: number
      hits: SearchHit[]
    }
    // query=economy (title/description) AND category=economy → only b/two
    expect(body.total_hits).toBe(1)
    expect(body.limit).toBe(1)
    expect(body.hits).toHaveLength(1)
    expect(body.hits[0].project_id).toBe('b/two')
  })
  test('empty params returns defaults', async () => {
    const res = await call('/api/v2/search')
    expect(res.status).toBe(200)
    const body = (await res.json()) as {
      offset: number
      limit: number
      total_hits: number
    }
    expect(body.offset).toBe(0)
    expect(body.limit).toBe(20)
    expect(body.total_hits).toBe(3)
  })
  test('dataset fetched from same-origin /v2/search.json with internal marker', async () => {
    const dataFetch = makeOriginFetch()
    await call('/api/v2/search', dataFetch)
    expect(dataFetch.requests).toHaveLength(1)
    expect(new URL(dataFetch.requests[0].url).pathname).toBe(
      '/api/v2/search.json',
    )
    expect(dataFetch.requests[0].headers.get('x-nh-internal')).toBe('1')
  })
  test('bad facets → 400 with message', async () => {
    const res = await call('/api/v2/search?facets=notjson')
    expect(res.status).toBe(400)
    const body = (await res.json()) as { error: string }
    expect(body.error).toBe('Bad Request')
  })
  test('dataset fetch failure → 502', async () => {
    const brokenFetch: DatasetFetch = () =>
      Promise.resolve(new Response('missing', { status: 404 }))
    const res = await call('/api/v2/search', brokenFetch)
    expect(res.status).toBe(502)
  })
})

describe('worker fetch handler: suffixless project routes', () => {
  const PROJECT_JSON = JSON.stringify({ id: 'a/one', title: 'One' })
  const VERSIONS_JSON = JSON.stringify([
    { version_number: 'v2', loaders: ['nkmot'], game_versions: ['1.0.0'] },
    { version_number: 'v1', loaders: ['pnx'], game_versions: ['2.0.0'] },
  ])
  const LATEST_JSON = JSON.stringify({ version_number: 'v2' })

  function projectFetch(): DatasetFetch & { requests: Request[] } {
    return makeOriginFetch({
      '/api/v2/project/a/one.json': PROJECT_JSON,
      '/api/v2/project/a/one/version.json': VERSIONS_JSON,
      '/api/v2/project/a/one/version/v2.json': JSON.stringify({
        version_number: 'v2',
      }),
      '/api/v2/project/a/one/latest.json': LATEST_JSON,
    })
  }

  test('two-segment id proxies the static project file', async () => {
    const res = await call('/api/v2/project/a/one', projectFetch())
    expect(res.status).toBe(200)
    expect(res.headers.get('access-control-allow-origin')).toBe('*')
    expect(await res.json()).toEqual({ id: 'a/one', title: 'One' })
  })
  test('%2F-encoded id (SDK-encoded) resolves to the same file', async () => {
    const res = await call('/api/v2/project/a%2Fone', projectFetch())
    expect(res.status).toBe(200)
    expect(await res.json()).toEqual({ id: 'a/one', title: 'One' })
  })
  test('single-segment slug resolves via the dataset (unique match)', async () => {
    const res = await call('/api/v2/project/one', projectFetch())
    expect(res.status).toBe(200)
    expect(await res.json()).toEqual({ id: 'a/one', title: 'One' })
  })
  test('unknown project → 404', async () => {
    const res = await call('/api/v2/project/no/such', projectFetch())
    expect(res.status).toBe(404)
  })
  test('traversal attempts → 404, never proxied', async () => {
    const dataFetch = projectFetch()
    // bare "../" is normalized away by URL parsing before routing; only the
    // %2F-encoded form actually reaches the router, and must be rejected
    const res = await call('/api/v2/project/a%2F..%2F..%2Fetc', dataFetch)
    expect(res.status).toBe(404)
    expect(dataFetch.requests).toHaveLength(0)
  })
  test('version list proxies the bare array when unfiltered', async () => {
    const dataFetch = projectFetch()
    const res = await call('/api/v2/project/a/one/version', dataFetch)
    expect(res.status).toBe(200)
    const body = (await res.json()) as { version_number: string }[]
    expect(body.map((v) => v.version_number)).toEqual(['v2', 'v1'])
    // unfiltered: streams the static file without inspecting it
    expect(dataFetch.requests).toHaveLength(1)
  })
  test('version list ?loaders= filter is applied server-side', async () => {
    const res = await call(
      '/api/v2/project/a/one/version?loaders=%5B%22pnx%22%5D',
      projectFetch(),
    )
    expect(res.status).toBe(200)
    const body = (await res.json()) as { version_number: string }[]
    expect(body.map((v) => v.version_number)).toEqual(['v1'])
  })
  test('version list ?game_versions= filter (AND with loaders)', async () => {
    const res = await call(
      '/api/v2/project/a/one/version?loaders=%5B%22nkmot%22%5D&game_versions=%5B%221.0.0%22%5D',
      projectFetch(),
    )
    const body = (await res.json()) as { version_number: string }[]
    expect(body.map((v) => v.version_number)).toEqual(['v2'])
  })
  test('version list malformed filter param → 400', async () => {
    const res = await call('/api/v2/project/a/one/version?loaders=oops')
    expect(res.status).toBe(400)
  })
  test('single version proxies by version slug', async () => {
    const res = await call('/api/v2/project/a/one/version/v2', projectFetch())
    expect(res.status).toBe(200)
  })
  test('version slug with unsafe characters → 404', async () => {
    const res = await call('/api/v2/project/a/one/version/.%2Fetc')
    expect(res.status).toBe(404)
  })
  test('latest proxies the latest file', async () => {
    const res = await call('/api/v2/project/a/one/latest', projectFetch())
    expect(res.status).toBe(200)
    expect(await res.json()).toEqual({ version_number: 'v2' })
  })
})

describe('worker fetch handler: suffixless version routes', () => {
  function versionFetch(): DatasetFetch & { requests: Request[] } {
    return makeOriginFetch({
      '/api/v2/project/a/one/version/v2.json': JSON.stringify({
        id: 'a/one@v2',
        project_id: 'a/one',
        version_number: 'v2',
      }),
      '/api/v2/project/b/two/version/v9.json': JSON.stringify({
        id: 'b/two@v9',
      }),
    })
  }

  test('/version/{version_id} proxies the static version file', async () => {
    const dataFetch = versionFetch()
    const res = await call('/api/v2/version/a%2Fone@v2', dataFetch)
    expect(res.status).toBe(200)
    expect(await res.json()).toEqual({
      id: 'a/one@v2',
      project_id: 'a/one',
      version_number: 'v2',
    })
    expect(new URL(dataFetch.requests[0].url).pathname).toBe(
      '/api/v2/project/a/one/version/v2.json',
    )
  })
  test('slash-decoded and %40-encoded id forms resolve to the same file', async () => {
    const decoded = await call('/api/v2/version/a/one@v2', versionFetch())
    expect(decoded.status).toBe(200)
    const encoded = await call('/api/v2/version/a%2Fone%40v2', versionFetch())
    expect(encoded.status).toBe(200)
    expect(await encoded.json()).toMatchObject({ id: 'a/one@v2' })
  })
  test('bare version number → 404 (only unique within a project)', async () => {
    const dataFetch = versionFetch()
    const res = await call('/api/v2/version/v2', dataFetch)
    expect(res.status).toBe(404)
    expect(dataFetch.requests).toHaveLength(0)
  })
  test('single-segment project form and traversal → 404, never proxied', async () => {
    const dataFetch = versionFetch()
    for (const path of [
      '/api/v2/version/one@v2',
      '/api/v2/version/a%2F..%40v2',
    ]) {
      const res = await call(path, dataFetch)
      expect(res.status).toBe(404)
    }
    expect(dataFetch.requests).toHaveLength(0)
  })
  test('unknown version → 404 via the static origin', async () => {
    const res = await call('/api/v2/version/a%2Fone@v999', versionFetch())
    expect(res.status).toBe(404)
  })
  test('/versions?ids= keeps order, skips unresolvable ids', async () => {
    const dataFetch = versionFetch()
    const ids = encodeURIComponent(
      JSON.stringify(['a%2Fone@v2', 'v9', 'a/one@nope', 'b%2Ftwo@v9']),
    )
    const res = await call(`/api/v2/versions?ids=${ids}`, dataFetch)
    expect(res.status).toBe(200)
    const body = (await res.json()) as { id: string }[]
    expect(body.map((v) => v.id)).toEqual(['a/one@v2', 'b/two@v9'])
  })
  test('/versions without ids / malformed / over 20 → 400', async () => {
    const fetch_ = versionFetch()
    expect((await call('/api/v2/versions', fetch_)).status).toBe(400)
    expect((await call('/api/v2/versions?ids=notjson', fetch_)).status).toBe(
      400,
    )
    expect((await call('/api/v2/versions?ids=%5B1%5D', fetch_)).status).toBe(
      400,
    )
    const many = encodeURIComponent(
      JSON.stringify(Array.from({ length: 21 }, () => 'a%2Fone@v2')),
    )
    expect((await call(`/api/v2/versions?ids=${many}`, fetch_)).status).toBe(
      400,
    )
  })
})

describe('worker fetch handler: batch / tags / meta', () => {
  test('/projects?ids= mixes two-segment ids and slugs, skips unknown', async () => {
    const dataFetch = makeOriginFetch({
      '/api/v2/project/a/one.json': JSON.stringify({ id: 'a/one' }),
      '/api/v2/project/b/two.json': JSON.stringify({ id: 'b/two' }),
    })
    const ids = encodeURIComponent('["a/b-typo","b/two","one"]')
    const res = await call(`/api/v2/projects?ids=${ids}`, dataFetch)
    expect(res.status).toBe(200)
    const body = (await res.json()) as { id: string }[]
    expect(body.map((p) => p.id)).toEqual(['b/two', 'a/one'])
  })
  test('/projects?ids= accepts %2F-encoded ids inside the JSON array', async () => {
    const dataFetch = makeOriginFetch({
      '/api/v2/project/a/one.json': JSON.stringify({ id: 'a/one' }),
    })
    const ids = encodeURIComponent('["a%2Fone"]')
    const res = await call(`/api/v2/projects?ids=${ids}`, dataFetch)
    expect(res.status).toBe(200)
    const body = (await res.json()) as { id: string }[]
    expect(body.map((p) => p.id)).toEqual(['a/one'])
  })
  test('/projects without ids → 400', async () => {
    const res = await call('/api/v2/projects')
    expect(res.status).toBe(400)
  })
  test('/projects with malformed ids → 400', async () => {
    const res = await call('/api/v2/projects?ids=notjson')
    expect(res.status).toBe(400)
    const res2 = await call('/api/v2/projects?ids=%5B1%5D')
    expect(res2.status).toBe(400)
  })
  test('/tag/{name} proxies known tags, 404s unknown', async () => {
    const dataFetch = makeOriginFetch({
      '/api/v2/tag/loader.json': JSON.stringify([{ name: 'nkmot' }]),
    })
    const ok = await call('/api/v2/tag/loader', dataFetch)
    expect(ok.status).toBe(200)
    expect(await ok.json()).toEqual([{ name: 'nkmot' }])
    const missing = await call('/api/v2/tag/license', dataFetch)
    expect(missing.status).toBe(404)
  })
  test('/meta proxies the static meta file', async () => {
    const dataFetch = makeOriginFetch({
      '/api/v2/meta.json': JSON.stringify({ api_base: 'x' }),
    })
    const res = await call('/api/v2/meta', dataFetch)
    expect(res.status).toBe(200)
    expect(await res.json()).toEqual({ api_base: 'x' })
  })
})

describe('worker fetch handler: routing guardrails', () => {
  test('static .json paths reaching the function → 508 (misconfiguration)', async () => {
    for (const path of [
      '/api/v2/search.json',
      '/api/v2/meta.json',
      '/api/v2/project/a/one.json',
      '/api/v2/search/lumi.json',
    ]) {
      const res = await call(path)
      expect(res.status).toBe(508)
    }
  })
  test('paths outside /api/v2 → 404 (function must not proxy)', async () => {
    for (const path of [
      '/api/v1/search',
      '/api/search',
      '/',
      '/api/v2',
      '/api/v2/',
    ]) {
      const res = await call(path)
      expect(res.status).toBe(404)
    }
  })
  test('internal marker header → 508 loop detection', async () => {
    const res = await handleRequest(
      new Request('https://plugins.nukkit-mot.com/api/v2/search', {
        headers: { 'x-nh-internal': '1' },
      }),
      makeOriginFetch(),
    )
    expect(res.status).toBe(508)
  })
  test('non-GET methods → 405', async () => {
    const res = await handleRequest(
      new Request('https://plugins.nukkit-mot.com/api/v2/search', {
        method: 'POST',
      }),
      makeOriginFetch(),
    )
    expect(res.status).toBe(405)
  })
  test('HEAD is allowed', async () => {
    const res = await handleRequest(
      new Request('https://plugins.nukkit-mot.com/api/v2/search', {
        method: 'HEAD',
      }),
      makeOriginFetch(),
    )
    expect(res.status).toBe(200)
  })
})
