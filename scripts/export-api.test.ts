/**
 * Unit tests for scripts/export-api.ts pure helpers.
 * Run with: bun test scripts/export-api.test.ts
 */

/// <reference types="bun-types" />
import { describe, expect, test } from 'bun:test'

import type AllayIndex from '../src/types/allayhub-index'
import {
  cleanPlugin,
  compareVersionStrings,
  isTemplatePlaceholder,
  mapProject,
  mapSearchHit,
  mapVersion,
  pickLatestVersion,
  resolveVersionSlugs,
  sortVersionsDesc,
  toIso,
  toVersionId,
  toVersionSlug
} from './export-api'

function makePlugin(
  overrides: Partial<AllayIndex.Plugin> = {},
): AllayIndex.Plugin {
  return {
    id: 'SoBadFish/BedWar',
    name: 'BedWar',
    source: 'https://github.com/SoBadFish/BedWar',
    summary: 'A bedwars minigame plugin',
    description: '# BedWar\nREADME body',
    authors: [
      {
        name: 'SoBadFish',
        url: 'https://github.com/SoBadFish',
        avatar_url: 'https://avatars.example/u/1',
      },
    ],
    categories: ['game-mechanics'],
    license: {
      id: 'MIT',
      name: 'MIT License',
      url: 'https://opensource.org/licenses/MIT',
    },
    links: {
      homepage: 'https://example.com',
      wiki: '',
      discord: 'https://discord.gg/x',
    },
    icon_url: 'https://example.com/icon.png',
    gallery: [
      {
        url: 'https://example.com/shot.png',
        title: 'icon',
        description: '',
        created: '2026-07-13',
      },
    ],
    downloads: 0,
    stars: 12,
    created_at: 1620000000,
    updated_at: 1724731298,
    versions: [
      {
        version: 'v2.2.3',
        name: 'v2.2.3 更新',
        prerelease: false,
        changelog: '+ fix',
        files: [
          {
            filename: 'BedWar_v2.2.3.jar',
            url: 'https://github.com/SoBadFish/BedWar/releases/download/v2.2.3/BedWar_v2.2.3.jar',
            size: 437018,
            primary: true,
          },
        ],
        downloads: 0,
        published_at: 1724731298,
      },
      {
        version: 'v2.1.0-beta',
        name: 'beta',
        prerelease: true,
        changelog: '',
        files: [
          {
            filename: 'b.jar',
            url: 'https://github.com/SoBadFish/BedWar/releases/download/v2.1.0-beta/b.jar',
            size: 1,
            primary: true,
          },
        ],
        downloads: 0,
        published_at: 1700000000,
      },
    ],
    targets: ['nkx', 'nkmot'],
    primary_target: 'nkx',
    api_version: '1.0.0',
    server_version: '',
    dependencies: [
      {
        plugin_id: 'EconomyAPI',
        version_range: '',
        dependency_type: 'optional',
      },
    ],
    ...overrides,
  }
}

describe('isTemplatePlaceholder / cleanPlugin', () => {
  test('detects ${...} and @...@ placeholders', () => {
    expect(isTemplatePlaceholder('${version}')).toBe(true)
    expect(isTemplatePlaceholder('@VERSION@')).toBe(true)
    expect(isTemplatePlaceholder('normal text')).toBe(false)
    expect(isTemplatePlaceholder(42)).toBe(false)
  })

  test('restores "!key" fallback entries when the field is missing/undefined', () => {
    const raw = {
      id: 'a/b',
      name: 'B',
      description: '',
      '!summary': 'fallback summary', // no "summary" sibling at all
      changelog: undefined,
      '!changelog': 'fallback changelog',
    } as unknown as AllayIndex.Plugin
    const cleaned = cleanPlugin(raw) as unknown as Record<string, unknown>
    expect(cleaned.summary).toBe('fallback summary')
    expect(cleaned.changelog).toBe('fallback changelog')
  })

  test('an existing non-null value wins over the "!key" fallback', () => {
    const raw = {
      id: 'a/b',
      name: 'B',
      summary: 'real summary',
      '!summary': 'fallback summary',
    } as unknown as AllayIndex.Plugin
    const cleaned = cleanPlugin(raw) as unknown as Record<string, unknown>
    expect(cleaned.summary).toBe('real summary')
  })

  test('falls back to repo name when name is a placeholder', () => {
    const cleaned = cleanPlugin(makePlugin({ name: '${project.name}' }))
    expect(cleaned.name).toBe('BedWar')
  })

  test('clears placeholder summaries', () => {
    const cleaned = cleanPlugin(makePlugin({ summary: '@DESCRIPTION@' }))
    expect(cleaned.summary).toBe('')
  })
})

describe('toIso', () => {
  test('converts unix seconds to ISO 8601 UTC', () => {
    expect(toIso(1724731298)).toBe('2024-08-27T04:01:38.000Z')
  })
  test('invalid input becomes epoch', () => {
    expect(toIso(undefined)).toBe('1970-01-01T00:00:00.000Z')
    expect(toIso(Number.NaN)).toBe('1970-01-01T00:00:00.000Z')
    expect(toIso('nope')).toBe('1970-01-01T00:00:00.000Z')
  })
})

describe('toVersionSlug / resolveVersionSlugs', () => {
  test('safe versions pass through', () => {
    for (const v of ['v2.2.3', '1.0.0', '1.0.0+build.5', '2-beta.1', '1_0']) {
      expect(toVersionSlug(v)).toBe(v)
    }
  })
  test('unsafe characters become dashes and leading symbol gets v prefix', () => {
    expect(toVersionSlug('beta/1')).toBe('beta-1')
    expect(toVersionSlug('1.0 beta')).toBe('1.0-beta')
    expect(toVersionSlug('.1.0')).toBe('v.1.0')
  })
  test('slugs are unique per plugin and duplicates stay idempotent', () => {
    const { slugByVersion } = resolveVersionSlugs([
      { version: 'beta/1' },
      { version: 'beta-1' },
      { version: 'beta-1' }, // duplicate version string
    ])
    // "beta/1" slugs to "beta-1", colliding with the literal "beta-1"
    expect(slugByVersion.get('beta/1')).toBe('beta-1')
    expect(slugByVersion.get('beta-1')).toBe('beta-1-2')
    const slugs = [...slugByVersion.values()]
    expect(new Set(slugs.map((s) => s.toLowerCase())).size).toBe(slugs.length)
  })
})

describe('sortVersionsDesc / pickLatestVersion', () => {
  test('sorts newest first, deterministic on ties', () => {
    const sorted = sortVersionsDesc([
      { published_at: 100, version: 'b' },
      { published_at: 300, version: 'c' },
      { published_at: 100, version: 'a' },
    ])
    expect(sorted.map((v) => v.version)).toEqual(['c', 'b', 'a'])
  })
  test('picks first non-prerelease, else newest', () => {
    expect(
      pickLatestVersion([{ prerelease: true }, { prerelease: false }]),
    ).toMatchObject({ prerelease: false })
    expect(
      pickLatestVersion([{ prerelease: true }, { prerelease: true }]),
    ).toMatchObject({ prerelease: true })
    expect(pickLatestVersion([])).toBeUndefined()
  })
})

describe('toVersionId', () => {
  test('composes globally-unique owner/name@version ids', () => {
    expect(toVersionId('SoBadFish/BedWar', 'v2.2.3')).toBe(
      'SoBadFish/BedWar@v2.2.3',
    )
    expect(toVersionId('a/repo--module', 'v-1_2')).toBe('a/repo--module@v-1_2')
  })
})

describe('mapVersion', () => {
  test('maps fields and derives version_type', () => {
    const plugin = makePlugin()
    const mapped = mapVersion(plugin.versions[0], 'v2.2.3', plugin)
    expect(mapped.id).toBe('SoBadFish/BedWar@v2.2.3')
    expect(mapped.project_id).toBe('SoBadFish/BedWar')
    expect(mapped.version_number).toBe('v2.2.3')
    expect(mapped.version_type).toBe('release')
    expect(mapped.date_published).toBe('2024-08-27T04:01:38.000Z')
    expect(mapped.files[0]).toMatchObject({
      filename: 'BedWar_v2.2.3.jar',
      primary: true,
      size: 437018,
      hashes: {},
    })
    // loaders/game_versions are inherited from the project
    expect(mapped.loaders).toEqual(['nkx', 'nkmot'])
    expect(mapped.game_versions).toEqual(['1.0.0'])
    const beta = mapVersion(plugin.versions[1], 'v2.1.0-beta', plugin)
    expect(beta.version_type).toBe('beta')
  })
})

describe('mapSearchHit', () => {
  test('maps Modrinth-shaped hit with extensions', () => {
    const hit = mapSearchHit(makePlugin())
    expect(hit).toMatchObject({
      project_id: 'SoBadFish/BedWar',
      slug: 'BedWar',
      project_type: 'plugin',
      title: 'BedWar',
      description: 'A bedwars minigame plugin',
      author: 'SoBadFish',
      categories: ['game-mechanics'],
      loaders: ['nkx', 'nkmot'],
      game_versions: ['1.0.0'],
      versions: ['SoBadFish/BedWar@v2.2.3', 'SoBadFish/BedWar@v2.1.0-beta'],
      latest_version: 'v2.2.3',
      date_created: '2021-05-03T00:00:00.000Z',
      date_modified: '2024-08-27T04:01:38.000Z',
      stars: 12,
    })
  })
  test('plugin without releases has empty latest_version and versions', () => {
    const hit = mapSearchHit(makePlugin({ versions: [] }))
    expect(hit.versions).toEqual([])
    expect(hit.latest_version).toBe('')
  })
  test('versions/latest_version use path-safe slugs matching the version endpoint', () => {
    const plugin = makePlugin({
      versions: [
        {
          version: 'beta/1',
          name: 'x',
          prerelease: false,
          changelog: '',
          files: [],
          downloads: 0,
          published_at: 100,
        },
      ],
    })
    const { slugByVersion } = resolveVersionSlugs(plugin.versions)
    const hit = mapSearchHit(plugin, slugByVersion)
    expect(hit.versions).toEqual(['SoBadFish/BedWar@beta-1'])
    expect(hit.latest_version).toBe('beta-1')
  })
})

describe('mapProject', () => {
  test('maps detail with derived GitHub links and extensions', () => {
    const plugin = makePlugin()
    const sorted = sortVersionsDesc(plugin.versions)
    const { slugByVersion } = resolveVersionSlugs(sorted)
    const project = mapProject(plugin, sorted, slugByVersion)
    expect(project).toMatchObject({
      id: 'SoBadFish/BedWar',
      slug: 'BedWar',
      project_type: 'plugin',
      title: 'BedWar',
      body: '# BedWar\nREADME body',
      loaders: ['nkx', 'nkmot'],
      versions: ['SoBadFish/BedWar@v2.2.3', 'SoBadFish/BedWar@v2.1.0-beta'],
      issues_url: 'https://github.com/SoBadFish/BedWar/issues',
      wiki_url: 'https://github.com/SoBadFish/BedWar/wiki',
      source_url: 'https://github.com/SoBadFish/BedWar',
      discord_url: 'https://discord.gg/x',
      homepage_url: 'https://example.com',
      api_version: '1.0.0',
      stars: 12,
      published: '2021-05-03T00:00:00.000Z',
      updated: '2024-08-27T04:01:38.000Z',
    })
    expect(project.gallery[0]).toMatchObject({
      url: 'https://example.com/shot.png',
      created: '2026-07-13',
    })
    expect(project.dependencies[0]).toMatchObject({
      plugin_id: 'EconomyAPI',
      dependency_type: 'optional',
    })
  })
  test('explicit wiki link wins over derived one', () => {
    const project = mapProject(
      makePlugin({
        links: { homepage: '', wiki: 'https://wiki.example.com', discord: '' },
      }),
      [],
      new Map(),
    )
    expect(project.wiki_url).toBe('https://wiki.example.com')
  })
  test('non-http URL schemes are stripped (javascript: injection)', () => {
    const project = mapProject(
      makePlugin({
        icon_url: 'javascript:alert(1)',
        links: {
          homepage: 'javascript:alert(2)',
          wiki: '',
          discord: 'ftp://evil',
        },
        gallery: [
          {
            url: 'javascript:alert(3)',
            title: '',
            description: '',
            created: '',
          },
        ],
        authors: [
          {
            name: 'x',
            url: 'javascript:alert(4)',
            avatar_url: 'javascript:alert(5)',
          },
        ],
      }),
      [],
      new Map(),
    )
    expect(project.icon_url).toBe('')
    expect(project.raw_icon_url).toBe('')
    expect(project.homepage_url).toBe('')
    expect(project.discord_url).toBe('')
    expect(project.gallery[0].url).toBe('')
    expect(project.authors[0].url).toBe('')
    expect(project.authors[0].avatar_url).toBe('')
  })
})

describe('compareVersionStrings', () => {
  test('numeric-aware ordering', () => {
    const versions = ['10.0.0', '2.0.0', '1.10.0', '1.9.0', '1.0.0']
    expect([...versions].sort(compareVersionStrings)).toEqual([
      '1.0.0',
      '1.9.0',
      '1.10.0',
      '2.0.0',
      '10.0.0',
    ])
  })
})
