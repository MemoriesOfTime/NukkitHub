/**
 * NukkitHub Public API v2 data types
 *
 * The public DTO contract. Shapes follow
 * Modrinth API v2 (fields renamed accordingly, ISO 8601 timestamps); fields
 * we have no equivalent for are omitted rather than filled with placeholders,
 * and NukkitHub-specific extensions are marked below.
 *
 * `/v2/` is the first (and only) published API version. The number matches
 * Modrinth's on purpose — tools that let you point a Modrinth client at a
 * custom base URL address `{api_base}/v2/...` directly. Additive-only within
 * `/v2/`; breaking changes will go to `/v3/`.
 *
 * This file is the single source of truth for scripts/export-api.ts output
 * and for the examples in docs/API.md.
 */

declare namespace ApiV2 {
  interface License {
    /** SPDX identifier, e.g. "MIT"; "ARR" when no open-source license detected */
    id: string
    name: string
    url: string
  }

  interface Author {
    name: string
    /** GitHub profile URL */
    url: string
    avatar_url: string
  }

  /** Near pass-through of the index GalleryImage (Modrinth-shaped) */
  interface GalleryImage {
    url: string
    title: string
    description: string
    /** ISO 8601 date string */
    created: string
  }

  /** Near pass-through of the index Dependency (Modrinth-shaped) */
  interface Dependency {
    /** Plugin name only ("EconomyAPI") — resolved IDs are a future addition */
    plugin_id: string
    version_range: string
    dependency_type: 'required' | 'optional'
  }

  /** Entry of GET {api_base}/v2/search.json — mirrors Modrinth's search hit */
  interface SearchHit {
    /** "owner/name" (two segments, unlike Modrinth's base62 ids) */
    project_id: string
    project_type: 'plugin'
    /** Repository (or repo--module) name segment of the id */
    slug: string
    /** First author's name */
    author: string
    title: string
    description: string
    categories: string[]
    loaders: string[]
    game_versions: string[]
    /** Version ids ("owner/name@version_number"), newest first */
    versions: string[]
    downloads: number
    icon_url: string
    /** ISO 8601 */
    date_created: string
    /** ISO 8601 */
    date_modified: string
    latest_version: string
    license: License
    // ---- NukkitHub extensions ----
    /** GitHub star count (approximates Modrinth's "follows") */
    stars: number
  }

  /** Response of GET {api_base}/v2/search.json and /v2/search/{loader}.json */
  interface SearchResponse {
    offset: number
    limit: number
    total_hits: number
    hits: SearchHit[]
  }

  /**
   * Response of GET {api_base}/v2/project/{owner}/{name}.json — bare object,
   * Modrinth-style. Also the element type of /version.json arrays.
   */
  interface Version {
    /**
     * Globally-unique version id: "owner/name@version_number" — version
     * numbers alone are only unique within a project. This is the id the
     * /v2/version/{version_id} endpoint accepts.
     */
    id: string
    /** Owning project id ("owner/name") */
    project_id: string
    /** Release title */
    name: string
    /**
     * Version string; the value to use in the /version/{version_number}
     * endpoint path (filename-unsafe characters are already replaced).
     */
    version_number: string
    changelog: string
    /** Derived: prerelease → "beta", otherwise "release" */
    version_type: 'release' | 'beta'
    /** ISO 8601 */
    date_published: string
    downloads: number
    /** Inherited from the project (versions carry no separate detection) */
    loaders: string[]
    /** Inherited from the project */
    game_versions: string[]
    files: VersionFile[]
    dependencies: Dependency[]
  }

  interface VersionFile {
    url: string
    filename: string
    /** Which jar a panel should install when a release has multiple assets */
    primary: boolean
    /** Bytes */
    size: number
    /**
     * Checksums keyed by algorithm ("sha256"); `{}` when the source provides
     * no digest (GitHub only exposes asset digests for recent uploads, and
     * CI-built artifacts carry none)
     */
    hashes: Record<string, string>
  }

  /** Response of GET {api_base}/v2/project/{owner}/{name}.json */
  interface Project {
    /** "owner/name" */
    id: string
    slug: string
    project_type: 'plugin'
    title: string
    description: string
    /** Full README body (raw markdown) */
    body: string
    categories: string[]
    loaders: string[]
    game_versions: string[]
    downloads: number
    icon_url: string
    raw_icon_url: string
    issues_url: string
    source_url: string
    wiki_url: string
    discord_url: string
    /** ISO 8601 */
    published: string
    /** ISO 8601 */
    updated: string
    /** Version ids ("owner/name@version_number"), newest first */
    versions: string[]
    license: License
    gallery: GalleryImage[]
    // ---- NukkitHub extensions ----
    authors: Author[]
    stars: number
    dependencies: Dependency[]
    homepage_url: string
    api_version: string
    server_version: string
  }

  interface LoaderTag {
    icon: string
    /** "nkmot" | "pnx" | "lumi" | "nkx" */
    name: string
    /** NukkitHub extension (Modrinth loader tags have no display name) */
    display_name: string
  }

  interface CategoryTag {
    icon: string
    name: string
    project_type: 'plugin'
  }

  interface GameVersionTag {
    version: string
    /** ISO 8601 date or empty (unknown) */
    date: string
  }

  /** Response of GET {api_base}/v2/meta.json (NukkitHub extension) */
  interface Meta {
    schema_version: string
    generated_at: string
    api_base: string
    site_url: string
    docs_url: string
    counts: {
      plugins: number
      plugins_with_downloads: number
      versions: number
      release_files: number
      /** Release files whose source provides a digest (populated in files[].hashes) */
      files_with_hashes: number
      by_loader: Record<string, number>
    }
    /** Field name → deprecation note; empty until a deprecation is announced */
    deprecated_fields: Record<string, string>
  }
}

export = ApiV2
export as namespace ApiV2
