# NukkitHub 公开 API（v2）

一个构建在 NukkitHub 插件索引之上的只读 JSON API，形状对齐
[Modrinth API v2](https://docs.modrinth.com/api/)。它存在的意义是让各类工具——
服务器面板安装器、机器人、包管理器——能够列出 Nukkit 系插件并获取可安装的
jar 下载地址。`/v2/` 前缀刻意与 Modrinth 一致：支持自定义 base URL 的
Modrinth 客户端直接指向 `{api_base}`，其 `/v2/…` 请求即可命中我们的路由
（见[将 Modrinth 工具指向 NukkitHub](#将-modrinth-工具指向-nukkithub)）。

- **规范 base URL**：`https://plugins.nukkit-mot.com/api`（下文记作 `{api_base}`）——
  与主站同源
- `meta.json` 会再次声明规范 `api_base`——长期寻址请优先读取它，而不是硬编码
  URL
- 所有响应均为 UTF-8 JSON，时间戳为 ISO 8601，CORS 开放
  （`Access-Control-Allow-Origin: *`）
- 任何时刻都可能出现未知字段——请忽略它们
- `/v2/` 是首个公开发布的版本；版本内只做增量变更，破坏性变更进入 `/v3/`

## 快速上手——面板安装流程（两次 GET）

```bash
# 1. 列出某个运行时的全部可安装插件
curl -s https://plugins.nukkit-mot.com/api/v2/search/nkmot.json \
  | jq '.hits[] | {project_id, title, latest_version}'

# 2. 解析所选插件的 jar 下载地址
curl -s https://plugins.nukkit-mot.com/api/v2/project/SoBadFish/BedWar/latest.json \
  | jq '.files[] | select(.primary) | .url'
```

需要更复杂的查询时，使用动态搜索（下文的 `/v2/search`）；只需要全量
清单时，拉取 `search.json`（几百 KiB）在客户端本地过滤即可。轮询更新时，
如果还想看到预发布版本，请轮询 `version.json` 而不是 `latest.json`；
轮询间隔不要短于 15 分钟——索引每小时重建，部署还会滞后于索引。

## 端点

| 端点                                                                     | 说明                                                                                         |
| ------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------- |
| `GET {api_base}/v2/search`                                               | **动态搜索**：`query`/`facets`/`index`/`offset`/`limit`                                      |
| `GET {api_base}/v2/projects?ids=["owner/name",…]`                        | 批量项目查询，≤ 20 个 id；接受 slug 与 `%2F` 编码 id                                         |
| `GET {api_base}/v2/project/{id}`                                         | 项目详情（裸对象）；`{id}` 为 `owner/name`、唯一单段 slug 或 `%2F` 编码形式                  |
| `GET {api_base}/v2/project/{id}/version`                                 | 版本列表（裸数组）；可选 `?loaders=` / `?game_versions=` JSON 数组过滤                       |
| `GET {api_base}/v2/project/{id}/version/{version_number}`                | 单个版本含文件                                                                               |
| `GET {api_base}/v2/version/{version_id}`                                 | 按全局唯一版本 id（`{project_id}@{version_number}`）取单个版本                               |
| `GET {api_base}/v2/versions?ids=["{version_id}",…]`                      | 批量版本查询，≤ 20 个 id；无法解析的 id 会被跳过                                             |
| `GET {api_base}/v2/version_file/{hash}`                                  | hash 反查（小写十六进制；索引携带 `sha256`），返回拥有该文件的版本；未知 hash 为 404         |
| `GET {api_base}/v2/project/{id}/latest`                                  | 最新可安装版本：首个非预发布版本，否则最新版本（插件无任何 release 时为 404）                |
| `GET {api_base}/v2/tag/{name}`                                           | 标签：`loader` / `category` / `game_version`                                                 |
| `GET {api_base}/v2/meta`                                                 | 索引元数据、计数、规范 `api_base`                                                            |
| `GET {api_base}/v2/search.json`                                          | 全部插件的 Modrinth 风格搜索响应（等价于无参数的 `/v2/search`），按 `date_modified` 降序排列 |
| `GET {api_base}/v2/search/{loader}.json`                                 | 相同形状，按 loader（`nkmot`、`pnx`、`lumi`、`nkx`）预过滤                                   |
| `GET {api_base}/v2/project/{owner}/{name}.json`                          | 项目详情含全部元数据；`versions` 为版本 id 数组，最新在前                                    |
| `GET {api_base}/v2/project/{owner}/{name}/version.json`                  | 版本列表（裸数组）                                                                           |
| `GET {api_base}/v2/project/{owner}/{name}/version/{version_number}.json` | 单个版本（裸对象）含文件                                                                     |
| `GET {api_base}/v2/version_file/{hash}.json`                             | hash 反查的静态形式（每个已索引 sha256 一个文件）                                            |
| `GET {api_base}/v2/project/{owner}/{name}/latest.json`                   | 最新可安装版本                                                                               |
| `GET {api_base}/v2/tag/{loader\|category\|game_version}.json`            | 标签文件                                                                                     |
| `GET {api_base}/v2/meta.json`                                            | 索引元数据、计数、规范 `api_base`                                                            |

`{owner}/{name}` 是从 GitHub 派生的 id（多模块仓库为
`owner/repo--module-suffix`）。没有已索引 GitHub Release 的插件 id，其
`versions` 数组为空，也没有 `latest`。访问 `{api_base}/` 可以看到
一个简短的人类可读端点索引页。

版本号只在**项目内**唯一，因此全局唯一的版本 id 是复合形式
`{project_id}@{version_number}`，例如 `SoBadFish/BedWar@v2.2.3`。`versions`
数组携带的就是这些 id；`/v2/version/{version_id}` 与 `/v2/versions?ids=[…]`
也接受它们（id 放进路径时需对 `/` 和 `@` 做 URL 编码，例如
`/v2/version/SoBadFish%2FBedWar%40v2.2.3`）。

## 动态搜索

`GET {api_base}/v2/search` 使用 Modrinth v2 的参数语法；不带任何参数
调用时，返回内容与 `search.json` 完全一致。

| 参数     | 默认值      | 说明                                                                                                                                                                                                                              |
| -------- | ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `query`  | _（无）_    | 对 `title` + `description` 的大小写不敏感子串匹配（最长 256 字符）                                                                                                                                                                |
| `facets` | _（无）_    | JSON 数组的数组——外层 AND，内层 OR。可用 key：`loaders`、`categories`、`versions`（匹配版本 id）、`game_versions`、`project_type`、`license`、`author`、`title`。未知 key 匹配不到任何结果（向前兼容）。最多 16 组 × 每组 16 项。 |
| `index`  | `relevance` | `relevance`（标题前缀 > 标题 > 描述，再按时间）、`updated`、`newest`、`stars`、`downloads`、`follows`——后两个在接入下载遥测之前以 `stars` 近似                                                                                    |
| `offset` | `0`         |                                                                                                                                                                                                                                   |
| `limit`  | `20`        | 最大 100                                                                                                                                                                                                                          |

```bash
curl -s 'https://plugins.nukkit-mot.com/api/v2/search?facets=%5B%5B%22loaders%3Ankmot%22%5D%5D&query=economy&index=stars&limit=10'
```

`facets` 格式错误时返回 `400` 及说明信息。响应携带
`Access-Control-Allow-Origin: *`、`Cache-Control: public, max-age=300` 和
`X-Content-Type-Options: nosniff`。支持 GET 与 HEAD，其余方法返回 `405`。

## 将 Modrinth 工具指向 NukkitHub

允许覆盖 API base URL 的 Modrinth SDK 和客户端可以直接对接 NukkitHub：
把 base 配置为 `https://plugins.nukkit-mot.com/api`，客户端发出的
`/v2/…` 请求路径就会落到我们的路由上。

开箱即用：

- `GET /v2/search`，完整参数语法，包括 `facets`
- `GET /v2/projects?ids=[…]` 批量查询（单次最多 20 个 id）
- `GET /v2/project/{id}`——`owner/name` id（包括 `%2F` 编码形式），
  以及单段 slug（当该 slug 唯一标识一个插件时）
- `GET /v2/project/{id}/version`，支持 `?loaders=` / `?game_versions=` 过滤
- `GET /v2/project/{id}/version/{version_number}` 与 `/latest`
- `GET /v2/version/{version_id}` 与 `GET /v2/versions?ids=[…]`（批量，单次
  最多 20 个 id）——版本 id 为 `{project_id}@{version_number}`
- `GET /v2/version_file/{sha256}`——hash 反查，返回包含该 sha256 文件的版本
- `GET /v2/tag/*`

设计上不可用：

- 鉴权、用户、团队、通知、收益类端点

## 响应形状

字段命名与结构遵循 Modrinth v2。快速概览：

```jsonc
// search.json（顶层）
{ "offset": 0, "limit": 359, "total_hits": 359, "hits": [ /* SearchHit */ ] }

// SearchHit（仅展示要点）
{
  "project_id": "SoBadFish/BedWar",  // "owner/name"——两段式，不是 base62
  "project_type": "plugin",
  "slug": "BedWar",
  "author": "SoBadFish",
  "title": "BedWar",
  "description": "…",
  "categories": ["game-mechanics"],
  "loaders": ["nkx", "nkmot"],       // 运行时目标
  "game_versions": ["1.0.11"],       // 服务端 API 版本
  "versions": ["SoBadFish/BedWar@v2.2.3", "…"],  // 版本 id（{project_id}@{version_number}）
  "latest_version": "v2.2.3",
  "downloads": 0,                     // 保留字段，当前恒为 0
  "icon_url": "…",
  "date_created": "2024-08-27T04:01:38.000Z",
  "date_modified": "2024-08-27T04:01:38.000Z",
  "license": { "id": "MIT", "name": "MIT License", "url": "…" },
  "stars": 12                         // NukkitHub 扩展（GitHub star 数）
}

// Version（version.json 的元素，或 /version/{n} 与 /latest 的响应体）
{
  "id": "SoBadFish/BedWar@v2.2.3",    // 全局唯一版本 id
  "project_id": "SoBadFish/BedWar",
  "name": "2023/08/27 v2.2.3 更新",
  "version_number": "v2.2.3",
  "changelog": "…",
  "version_type": "release",          // GitHub release 为预发布时是 "beta"
  "date_published": "2024-08-27T04:01:38.000Z",
  "downloads": 0,
  "loaders": ["nkx", "nkmot"],        // 从项目继承
  "game_versions": ["1.0.11"],
  "files": [
    {
      "url": "https://github.com/…/BedWar_v2.2.3.jar",
      "filename": "BedWar_v2.2.3.jar",
      "primary": true,                // 面板应当安装的那个 jar
      "size": 437018,
      "hashes": {                     // 按算法名组织的校验和
        "sha256": "9f86d0…"           // 来源无 digest 时为 {}
      }
    }
  ],
  "dependencies": [
    { "plugin_id": "EconomyAPI", "version_range": "", "dependency_type": "optional" }
  ]
}
```

项目详情对象额外携带 `body`（完整 README markdown）、`gallery`、`authors`、
`source_url`、`issues_url`、`wiki_url`、`discord_url`，以及 NukkitHub 扩展
字段（`stars`、`authors`、`homepage_url`、`api_version`、`server_version`、
`dependencies`）。

完整类型契约见
[`src/types/api-v2.d.ts`](https://github.com/MemoriesOfTime/NukkitHub/blob/master/src/types/api-v2.d.ts)。

## 与 Modrinth API v2 的差异

这是一个**形状兼容、寻址风格一致的子集，不是可直接替换的替代品**。
仍然存在的差异：

| Modrinth 能力                                      | 本 API                                                                           | 原因                                        |
| -------------------------------------------------- | -------------------------------------------------------------------------------- | ------------------------------------------- |
| `GET /v2/search?query=&facets=&offset=&limit=`     | 同语法的 `/v2/search`；facet key 与排序值为子集（见上文参数表）                  | 索引只有这些维度                            |
| `GET /v2/projects?ids=[…]`                         | 支持（≤ 20 个 id）                                                               | —                                           |
| `GET /v2/versions?ids=[…]`、`GET /v2/version/{id}` | 支持——版本 id 为复合形式 `{project_id}@{version_number}`（版本号只在项目内唯一） | 身份即 GitHub Release                       |
| `GET /v2/version_file/{hash}`                      | 支持 sha256（索引携带的唯一 hash）；sha1/sha512 长度的 hash 请求格式合法但恒 404 | 文件 digest 只有 GitHub 提供，且只有 sha256 |
| `featured` 标记 / `?featured` 过滤                 | 以 `latest` 路由替代                                                             | 源数据没有 featured 概念                    |
| `follows` / `followers`                            | 以扩展字段 `stars` 替代                                                          | 无遥测                                      |
| 8 位 base62 id、单段 slug                          | `owner/name` 两段式 id；单段 slug 唯一时可解析                                   | 身份即 GitHub 仓库                          |
| 鉴权、团队、通知、举报、收益                       | 无                                                                               | 只读公开数据，无用户系统                    |
| 限流头、强制 User-Agent                            | 不强制                                                                           | 仍建议附带可识别的 User-Agent               |

## 数据注意事项

- `downloads` 目前**恒为 0**（保留占位字段）。
- `files[].hashes` 在来源提供 digest 时携带 `sha256`。GitHub 仅对约 2025 年
  中之后上传的 Release 资产提供 digest；更早的资产与 `ci-{build}` CI 产物为
  `"hashes": {}`。覆盖率见 `meta.counts.files_with_hashes`。
- `version_number` 可直接用作项目内 `/project/{id}/version/{version_number}`
  的路径段；版本 id（`{project_id}@{version_number}`）的两部分也都已路径
  安全——文件名不安全字符已由导出器替换。
- `body` 和 `changelog` 是来自仓库 README / Release 说明的原始 markdown。
  渲染前请自行净化（NukkitHub 站点对它们同样处理）。
