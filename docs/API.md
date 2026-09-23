# NukkitHub Public API (v2)

A read-only JSON API over the NukkitHub plugin index, shaped after the
[Modrinth API v2](https://docs.modrinth.com/api/). It exists so that tools —
server-panel installers, bots, package managers — can list Nukkit-family
plugins and fetch installable jar URLs. The `/v2/` prefix deliberately
matches Modrinth's own: point a Modrinth client that accepts a custom base
URL at `{api_base}` and its `/v2/…` requests resolve here directly (see
[Pointing Modrinth tooling at NukkitHub](#pointing-modrinth-tooling-at-nukkithub)).

- **Canonical base URL**: `https://plugins.nukkit-mot.com/api` (`{api_base}` below) —
  same origin as the main site
- `meta.json` re-declares the canonical `api_base` — prefer it over hard-coded
  URLs for long-term addressing
- All responses are UTF-8 JSON, timestamps are ISO 8601, CORS is open
  (`Access-Control-Allow-Origin: *`)
- Unknown fields may appear at any time — ignore them
- `/v2/` is the first published version; changes within it are additive-only,
  breaking changes will go to `/v3/`

## Quick start — panel install flow (two GETs)

```bash
# 1. list installable plugins for one runtime
curl -s https://plugins.nukkit-mot.com/api/v2/search/nkmot.json \
  | jq '.hits[] | {project_id, title, latest_version}'

# 2. resolve the jar URL for a chosen plugin
curl -s https://plugins.nukkit-mot.com/api/v2/project/SoBadFish/BedWar/latest.json \
  | jq '.files[] | select(.primary) | .url'
```

For richer queries use dynamic search (`/v2/search` below); to work from a
full list, pull `search.json` (a few hundred KiB) and filter client-side.
Poll `version.json` for updates rather than `latest.json`
if you also want to see prereleases; poll at most every 15 minutes — the index
rebuilds hourly and deploys trail it.

## Endpoints

| Endpoint                                                                 | Description                                                                                                             |
| ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| `GET {api_base}/v2/search`                                               | **Dynamic search**: `query`/`facets`/`index`/`offset`/`limit`                                                           |
| `GET {api_base}/v2/projects?ids=["owner/name",…]`                        | Batch project lookup, ≤ 20 ids; slugs and `%2F`-encoded ids accepted                                                    |
| `GET {api_base}/v2/project/{id}`                                         | Project detail (bare object); `{id}` is `owner/name`, a unique single-segment slug, or `%2F`-encoded                    |
| `GET {api_base}/v2/project/{id}/version`                                 | Version list (bare array); optional `?loaders=` / `?game_versions=` JSON-array filters                                  |
| `GET {api_base}/v2/project/{id}/version/{version_number}`                | Single version with files                                                                                               |
| `GET {api_base}/v2/version/{version_id}`                                 | Single version by globally-unique version id (`{project_id}@{version_number}`)                                          |
| `GET {api_base}/v2/versions?ids=["{version_id}",…]`                      | Batch version lookup, ≤ 20 ids; unresolvable ids are skipped                                                            |
| `GET {api_base}/v2/project/{id}/latest`                                  | Latest installable version: first non-prerelease, else newest (404 if the plugin has no releases)                       |
| `GET {api_base}/v2/tag/{name}`                                           | Tags: `loader` / `category` / `game_version`                                                                            |
| `GET {api_base}/v2/meta`                                                 | Index metadata, counts, canonical `api_base`                                                                            |
| `GET {api_base}/v2/search.json`                                          | Every plugin in one Modrinth-style search response (a parameterless `/v2/search`), sorted by `date_modified` descending |
| `GET {api_base}/v2/search/{loader}.json`                                 | Same shape, pre-filtered by loader (`nkmot`, `pnx`, `lumi`, `nkx`)                                                      |
| `GET {api_base}/v2/project/{owner}/{name}.json`                          | Project detail with all metadata; `versions` lists version ids newest-first                                             |
| `GET {api_base}/v2/project/{owner}/{name}/version.json`                  | Version list (bare array)                                                                                               |
| `GET {api_base}/v2/project/{owner}/{name}/version/{version_number}.json` | Single version with files                                                                                               |
| `GET {api_base}/v2/project/{owner}/{name}/latest.json`                   | Latest installable version                                                                                              |
| `GET {api_base}/v2/tag/{loader\|category\|game_version}.json`            | Tag files                                                                                                               |
| `GET {api_base}/v2/meta.json`                                            | Index metadata, counts, canonical `api_base`                                                                            |

`{owner}/{name}` is the GitHub-derived id (multi-module repositories use
`owner/repo--module-suffix`). Plugin ids with no indexed GitHub Release simply
have an empty `versions` array and no `latest`. Visiting `{api_base}/` serves
a small human-readable endpoint index page.

Version numbers are only unique **within** a project, so the globally-unique
version id is the composite `{project_id}@{version_number}` — for example
`SoBadFish/BedWar@v2.2.3`. `versions` arrays carry these ids, and
`/v2/version/{version_id}` and `/v2/versions?ids=[…]` accept them (URL-encode
the `/` and the `@` when an id goes into a path, e.g.
`/v2/version/SoBadFish%2FBedWar%40v2.2.3`).

## Dynamic search

`GET {api_base}/v2/search` takes the Modrinth v2 parameter syntax; called
with no parameters it returns exactly what `search.json` contains.

| Parameter | Default     | Notes                                                                                                                                                                                                                                                  |
| --------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `query`   | _(none)_    | Case-insensitive substring match over `title` + `description` (max 256 chars)                                                                                                                                                                          |
| `facets`  | _(none)_    | JSON array of arrays — outer AND, inner OR. Keys: `loaders`, `categories`, `versions` (matches version ids), `game_versions`, `project_type`, `license`, `author`, `title`. Unknown keys match nothing (forward compatible). Max 16 groups x 16 terms. |
| `index`   | `relevance` | `relevance` (title prefix > title > description, then recency), `updated`, `newest`, `stars`, `downloads`, `follows` — the last two are approximated by `stars` until download telemetry exists                                                        |
| `offset`  | `0`         |                                                                                                                                                                                                                                                        |
| `limit`   | `20`        | Max 100                                                                                                                                                                                                                                                |

```bash
curl -s 'https://plugins.nukkit-mot.com/api/v2/search?facets=%5B%5B%22loaders%3Ankmot%22%5D%5D&query=economy&index=stars&limit=10'
```

Malformed `facets` returns `400` with an explanatory message. Responses carry
`Access-Control-Allow-Origin: *`, `Cache-Control: public, max-age=300` and
`X-Content-Type-Options: nosniff`. GET and HEAD are supported; other methods
return `405`.

## Pointing Modrinth tooling at NukkitHub

Modrinth SDKs and clients that let you override the API base URL can talk to
NukkitHub directly: configure the base as `https://plugins.nukkit-mot.com/api`
so the client's `/v2/…` request paths land on our routes.

Works out of the box:

- `GET /v2/search` with the full parameter syntax, including `facets`
- `GET /v2/projects?ids=[…]` batch lookup (max 20 ids per call)
- `GET /v2/project/{id}` — `owner/name` ids, including `%2F`-encoded forms,
  plus single-segment slugs (when the slug identifies exactly one plugin)
- `GET /v2/project/{id}/version` with `?loaders=` / `?game_versions=` filters
- `GET /v2/project/{id}/version/{version_number}` and `/latest`
- `GET /v2/version/{version_id}` and `GET /v2/versions?ids=[…]` (batch, max 20
  ids) — version ids are `{project_id}@{version_number}`
- `GET /v2/tag/*`

Does not work, by design:

- `GET /v2/version_file/{hash}` — `files[].hashes` is always `{}` today
- authentication, user, team, notification, and payout endpoints

## Shapes

The field names and structure follow Modrinth v2. Quick orientation:

```jsonc
// search.json (top level)
{ "offset": 0, "limit": 359, "total_hits": 359, "hits": [ /* SearchHit */ ] }

// SearchHit (essentials only shown)
{
  "project_id": "SoBadFish/BedWar",  // "owner/name" — two segments, not base62
  "project_type": "plugin",
  "slug": "BedWar",
  "author": "SoBadFish",
  "title": "BedWar",
  "description": "…",
  "categories": ["game-mechanics"],
  "loaders": ["nkx", "nkmot"],       // runtime targets
  "game_versions": ["1.0.11"],       // server API versions
  "versions": ["SoBadFish/BedWar@v2.2.3", "…"],  // version ids ({project_id}@{version_number})
  "latest_version": "v2.2.3",
  "downloads": 0,                     // reserved, currently always 0
  "icon_url": "…",
  "date_created": "2024-08-27T04:01:38.000Z",
  "date_modified": "2024-08-27T04:01:38.000Z",
  "license": { "id": "MIT", "name": "MIT License", "url": "…" },
  "stars": 12                         // NukkitHub extension (GitHub stars)
}

// Version (element of version.json, or the /version/{n} and /latest bodies)
{
  "id": "SoBadFish/BedWar@v2.2.3",    // globally-unique version id
  "project_id": "SoBadFish/BedWar",
  "name": "2023/08/27 v2.2.3 更新",
  "version_number": "v2.2.3",
  "changelog": "…",
  "version_type": "release",          // "beta" when the GitHub release is a prerelease
  "date_published": "2024-08-27T04:01:38.000Z",
  "downloads": 0,
  "loaders": ["nkx", "nkmot"],        // inherited from the project
  "game_versions": ["1.0.11"],
  "files": [
    {
      "url": "https://github.com/…/BedWar_v2.2.3.jar",
      "filename": "BedWar_v2.2.3.jar",
      "primary": true,                // the jar a panel should install
      "size": 437018,
      "hashes": {}                    // reserved, currently always empty
    }
  ],
  "dependencies": [
    { "plugin_id": "EconomyAPI", "version_range": "", "dependency_type": "optional" }
  ]
}
```

Project detail objects additionally carry `body` (full README markdown),
`gallery`, `authors`, `source_url`, `issues_url`, `wiki_url`, `discord_url`,
and the NukkitHub extensions (`stars`, `authors`, `homepage_url`,
`api_version`, `server_version`, `dependencies`).

The full type contract lives in
[`src/types/api-v2.d.ts`](https://github.com/MemoriesOfTime/NukkitHub/blob/master/src/types/api-v2.d.ts).

## Differences from Modrinth API v2

This is a **shape-compatible subset with Modrinth-style addressing, not a
drop-in replacement**. What remains different:

| Modrinth capability                                | Here                                                                                                                       | Why                                           |
| -------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| `GET /v2/search?query=&facets=&offset=&limit=`     | `/v2/search` with the same syntax; facet keys and sort values are a subset (see the parameter table)                       | The index only carries those dimensions       |
| `GET /v2/projects?ids=[…]`                         | Supported (≤ 20 ids)                                                                                                       | —                                             |
| `GET /v2/versions?ids=[…]`, `GET /v2/version/{id}` | Supported — version ids are `{project_id}@{version_number}` composites (version numbers alone are only unique per project) | Identity is the GitHub release                |
| `GET /v2/version_file/{hash}`                      | No equivalent; `files[].hashes` is `{}`                                                                                    | The index carries no file hashes yet          |
| `featured` flag / `?featured` filter               | `latest` route instead                                                                                                     | No featured concept in the source data        |
| `follows` / `followers`                            | Extension field `stars` instead                                                                                            | No telemetry                                  |
| 8-char base62 ids, single-segment slugs            | `owner/name` two-segment ids; single-segment slugs resolve when unique                                                     | Identity is the GitHub repository             |
| Auth, teams, notifications, reports, payouts       | Not present                                                                                                                | Read-only public data, no user system         |
| Rate-limit headers, mandatory User-Agent           | None enforced                                                                                                              | A descriptive User-Agent is still appreciated |

## Data caveats

- `downloads` is **always 0** today (reserved placeholder).
- `files[].hashes` is **always `{}`** (reserved placeholder).
- `version_number` is usable directly as the project-scoped
  `/project/{id}/version/{version_number}` path segment; both parts of a
  version id (`{project_id}@{version_number}`) are already path-safe —
  filename-unsafe characters are replaced by the exporter.
- `body` and `changelog` are raw markdown from the repository README/release
  notes. Sanitize before rendering (the NukkitHub site treats them the same way).
