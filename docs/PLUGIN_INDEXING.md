# Plugin Indexing Guide

This document describes how AllayHub indexes Nukkit plugins from GitHub repositories. Follow these guidelines to ensure your plugin is properly discovered and displayed.

## Discovery Requirements

For your repository to be indexed as a Nukkit plugin, it must meet **all** of the following criteria:

1. **Public GitHub repository** - Private repositories are not indexed
2. **Not archived** - Archived repositories are excluded
3. **Not a template** - Template repositories are excluded
4. **Not a `noindex` topic** - Repositories with the `noindex` topic are excluded (see [Plugin Removal](#plugin-removal))
5. **Discoverable** - At least one of:
   - Contains `plugin.yml` in `src/main/resources/`
   - Has the `nukkit-plugin` or `nukkit-mot-plugin` topic on the repository

## Discovery Methods

AllayHub uses two complementary methods to discover plugins:

### 1. Code Search

Searches for repositories containing `plugin.yml` in the standard Nukkit resources directory. This is the primary discovery method.

> **Note:** GitHub's code search index may have delays or gaps, especially for:
> - Newly created repositories
> - Repositories with low activity
> - Repositories generated from templates

### 2. Topic Search (Recommended)

Searches for repositories with the `nukkit-plugin` or `nukkit-mot-plugin` topic. This method is more reliable because:

- Repository metadata is indexed faster than code content
- Works even if the code search index hasn't caught up
- Explicitly signals that the repository is a Nukkit plugin

**To add the topic:** Go to your repository → About (gear icon) → Topics → Add `nukkit-plugin`

> **Note:** Forked repositories are indexed only when discovered via the topic search method. If your plugin is a fork, add the `nukkit-plugin` topic to ensure it is discovered.

## Plugin ID Format

Each plugin is assigned an ID in the `owner/name` format (all lowercase), where:

- `owner` is the GitHub repository owner (user or organization)
- `name` is the plugin name (from `plugin.yml` or repository name as fallback)

For example, a plugin named "MyPlugin" in a repository owned by "CoolDev" would have the ID `cooldev/myplugin`.

Plugin data files are stored in a nested directory structure: `AllayHubIndex/{owner}/{name}.json`.

Frontend URLs follow the same pattern: `/plugin/{owner}/{name}`.

## Plugin Metadata

### plugin.yml (Required)

Define plugin metadata in `src/main/resources/plugin.yml`:

```yaml
name: MyNukkitPlugin
version: 1.0.0
main: com.example.MyNukkitPlugin
api: ["1.0.0"]
authors: ["AuthorName"]
description: A short description of my plugin
website: https://example.com
depend: []
softdepend: []
```

### Metadata Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | ✓ | Plugin name (used as plugin ID) |
| `version` | string | ✓ | Plugin version |
| `main` | string | ✓ | Main class path |
| `api` | string or array | ✓ | Nukkit API version(s) |
| `authors` | string or array | | Plugin author(s) |
| `description` | string | | Short description |
| `website` | string | | Plugin website URL |
| `depend` | array | | Required dependencies |
| `softdepend` | array | | Optional dependencies |

## Categories

Add category topics to your repository to help users discover your plugin:

| Category | Topic |
|----------|-------|
| Adventure | `nukkit-adventure` |
| Cursed | `nukkit-cursed` |
| Decoration | `nukkit-decoration` |
| Economy | `nukkit-economy` |
| Equipment | `nukkit-equipment` |
| Food | `nukkit-food` |
| Game Mechanics | `nukkit-game-mechanics` |
| Library | `nukkit-library` |
| Magic | `nukkit-magic` |
| Management | `nukkit-management` |
| Minigame | `nukkit-minigame` |
| Mobs | `nukkit-mobs` |
| Optimization | `nukkit-optimization` |
| Social | `nukkit-social` |
| Storage | `nukkit-storage` |
| Technology | `nukkit-technology` |
| Transportation | `nukkit-transportation` |
| Utility | `nukkit-utility` |
| World Generation | `nukkit-world-generation` |

## Versions

Only **GitHub Releases** are indexed as plugin versions. To publish a version:

1. Create a GitHub Release
2. Attach your plugin JAR file
3. The indexer will automatically detect and index it

Version metadata is extracted from:
- Release tag (version number)
- Release name (display name)
- Release body (changelog)
- Attached JAR files

## Custom Icon

Add a custom icon to make your plugin stand out:

1. Place `logo.png` or `icon.png` in `.github/img/` or repository root
2. Recommended size: 256x256 pixels
3. Format: PNG with transparency

If no custom icon is found, the repository owner's avatar is used.

## Gallery Images

Add screenshots to showcase your plugin:

1. Place PNG images in `.github/img/`
2. Images named `logo.png` or `icon.png` are excluded (used as icon)
3. All other PNG files become gallery images
4. Images are also extracted from README markdown

## README

Your README.md becomes the plugin's detailed description:

- Markdown is converted to HTML
- Images are processed and added to the gallery
- Links are converted to absolute URLs
- Supports standard markdown features

## License

The indexer detects your license from:

1. GitHub's license detection (preferred)
2. LICENSE file in repository root

Supported license types:
- Open source licenses (MIT, Apache, GPL, etc.)
- All Rights Reserved (if no license specified)

## Plugin Removal

Plugins are automatically removed from the index when any of the following conditions are met during the update cycle (runs hourly):

| Condition | Description |
|-----------|-------------|
| Repository deleted | The repository no longer exists (404) |
| Repository archived | The repository has been archived |
| Plugin removed | The `plugin.yml` no longer exists |
| Opted out | The repository has the `noindex` topic |

To manually remove your plugin from AllayHub, add the `noindex` topic to your repository (About → Topics → add `noindex`).

## Tips for Better Indexing

1. **Add `nukkit-plugin` topic** - Ensures your plugin is discovered even if code search hasn't indexed it yet
2. **Use category topics** - Add relevant category topics (see [Categories](#categories)) to your repository
3. **Write a good README** - The README becomes your plugin's detailed description
4. **Create releases** - Only released versions appear in the version list
5. **Add a logo** - A custom logo helps your plugin stand out
6. **Include gallery images** - Screenshots help users understand your plugin
7. **Fill out plugin.yml** - Complete metadata improves discoverability

## Example Repository Structure

```
my-nukkit-plugin/
├── .github/
│   └── img/
│       ├── logo.png            # Plugin icon
│       ├── gallery1.png        # Gallery image 1
│       └── gallery2.png        # Gallery image 2
├── src/
│   └── main/
│       ├── java/
│       │   └── com/example/
│       │       └── MyPlugin.java
│       └── resources/
│           └── plugin.yml      # Plugin metadata
├── README.md                   # Description
└── LICENSE                     # License file
```

## Troubleshooting

### My plugin isn't showing up

1. Check that your repository is public
2. Verify `plugin.yml` exists in `src/main/resources/`
3. Add the `nukkit-plugin` topic to your repository
4. Wait up to 1 hour for the next indexing cycle
5. Check that your repository isn't archived or excluded

### My plugin information is outdated

The indexer updates existing plugins hourly. Changes to your repository will be reflected within 1 hour.

### My releases aren't showing up

1. Ensure you're creating GitHub Releases (not just tags)
2. Attach JAR files to the release
3. Wait for the next update cycle (runs hourly)

## Support

For issues or questions:
- Open an issue on the AllayHub repository
- Check existing issues for similar problems
- Provide your repository URL and plugin ID
