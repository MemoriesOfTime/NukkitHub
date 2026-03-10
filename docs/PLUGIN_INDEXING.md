# Plugin Indexing Guide

This guide is written for plugin developers. If you want your project to appear on NukkitHub, follow the checklist below.

## Quick Inclusion Checklist

To get your plugin included, make sure your repository satisfies all of the following:

1. **Use a public GitHub repository**
2. **Do not archive the repository**
3. **Do not mark it as a template**
4. **Do not add the `noindex` topic**
5. **Put your manifest in the standard path**
   - `src/main/resources/plugin.yml`
   - or `src/main/resources/powernukkitx.yml` for PowerNukkitX-first modules
6. **Add the topic that matches your target runtime**
7. **Fill in `plugin.yml` / `powernukkitx.yml` and README.md**

If you only do one extra thing beyond the manifest, add the correct topic. That makes discovery much more reliable.

## Which Topic Should You Add?

Use the topic that best matches your plugin:

| Target runtime | Recommended topic                     |
| -------------- | ------------------------------------- |
| NukkitX        | `nukkit-plugin`                       |
| Nukkit-MOT     | `nukkit-mot-plugin`                   |
| PowerNukkitX   | `powernukkitx-plugin` or `pnx-plugin` |
| Lumi           | `lumi-plugin`                         |

If your plugin supports both NukkitX and Nukkit-MOT, `nukkit-plugin` is acceptable.

**How to add a topic:** GitHub repository -> About -> gear icon -> Topics.

> **Fork note:** If your plugin repository is a fork, adding the correct topic is especially important.

## What To Do For Each Runtime

### NukkitX (`nkx`)

Recommended setup:

- Use `src/main/resources/plugin.yml`
- Add the `nukkit-plugin` topic
- Reference NukkitX-related repositories or dependencies such as:
  - `cloudburstmc`
  - `opencollab.dev`
  - `repo.nukkitx.com`

If your plugin uses generic `cn.nukkit:*` dependencies, it may be treated as shared compatibility for both NukkitX and Nukkit-MOT.

### Nukkit-MOT (`nkmot`)

Recommended setup:

- Use `src/main/resources/plugin.yml`
- Add the `nukkit-mot-plugin` topic
- Reference Nukkit-MOT-related names in your build files, such as:
  - `memoriesoftime`
  - `nukkit-mot`

If your plugin supports both NukkitX and Nukkit-MOT, generic `cn.nukkit:*` dependencies are acceptable.

> **Important:** MOTCI is not a repository submission channel. Your plugin still needs to be discoverable from GitHub.

### PowerNukkitX (`pnx`)

Recommended setup:

- Prefer `src/main/resources/powernukkitx.yml`
- Add `powernukkitx-plugin` or `pnx-plugin`
- Reference PowerNukkitX-related dependencies, such as:
  - `cn.powernukkitx`
  - `powernukkitx`

Using `powernukkitx.yml` is the clearest way to make a PowerNukkitX module indexable.

### Lumi (`lumi`)

Recommended setup:

- Use `src/main/resources/plugin.yml`
- Add the `lumi-plugin` topic
- Reference Lumi in Gradle or Maven, such as:
  - `repo.luminiadev.com`
  - `com.koshakmine:lumi`
  - Maven coordinates containing `com.koshakmine` and `lumi`

## Manifest Requirements

At least one supported manifest must exist in the standard path:

- `src/main/resources/plugin.yml`
- `src/main/resources/powernukkitx.yml`

Example:

```yaml
name: MyNukkitPlugin
version: 1.0.0
main: com.example.MyNukkitPlugin
api: ['1.0.0']
authors: ['AuthorName']
description: A short description of my plugin
website: https://example.com
depend: []
softdepend: []
```

### Required Metadata

| Field         | Required | Description              |
| ------------- | -------- | ------------------------ |
| `name`        | Yes      | Plugin name              |
| `version`     | Yes      | Plugin version           |
| `main`        | Yes      | Main class path          |
| `api`         | Yes      | Supported API version(s) |
| `authors`     | No       | Author list              |
| `description` | No       | Short summary            |
| `website`     | No       | Homepage URL             |
| `depend`      | No       | Required dependencies    |
| `softdepend`  | No       | Optional dependencies    |

## Multi-Module Repositories

If your repository contains multiple plugin modules:

- each module should have its own manifest in `src/main/resources/`
- each module is indexed separately
- the generated IDs will look like `owner/repo--module-suffix`

Examples:

- Single-module repository: `cooldev/myplugin-repo`
- Multi-module repository: `cooldev/myplugin-repo--modules-economy`

## Versions And Downloads

Your repository can be indexed without a GitHub Release.

However, if you want users to see downloadable versions on NukkitHub, publish releases this way:

1. Create a GitHub Release
2. Attach one or more `.jar` files
3. Use the release title and body as your version title and changelog

Only **GitHub Releases** are indexed as plugin versions.

## Categories

Add category topics to help users find your plugin:

| Category         | Topic                     |
| ---------------- | ------------------------- |
| Adventure        | `nukkit-adventure`        |
| Cursed           | `nukkit-cursed`           |
| Decoration       | `nukkit-decoration`       |
| Economy          | `nukkit-economy`          |
| Equipment        | `nukkit-equipment`        |
| Food             | `nukkit-food`             |
| Game Mechanics   | `nukkit-game-mechanics`   |
| Library          | `nukkit-library`          |
| Magic            | `nukkit-magic`            |
| Management       | `nukkit-management`       |
| Minigame         | `nukkit-minigame`         |
| Mobs             | `nukkit-mobs`             |
| Optimization     | `nukkit-optimization`     |
| Social           | `nukkit-social`           |
| Storage          | `nukkit-storage`          |
| Technology       | `nukkit-technology`       |
| Transportation   | `nukkit-transportation`   |
| Utility          | `nukkit-utility`          |
| World Generation | `nukkit-world-generation` |

## README, Icon, And Gallery

To make your listing look complete after inclusion:

### README

Your `README.md` becomes the main long description.

Recommended:

- explain what the plugin does
- show usage screenshots
- include installation steps
- keep links valid and public

### Icon

To add a custom icon:

1. place `logo.png` or `icon.png` in `.github/img/` or the repository root
2. use PNG format
3. recommended size: `256x256`

If no icon is found, the repository owner's avatar is used.

### Gallery Images

To add screenshots:

1. place PNG files in `.github/img/`
2. do not use `logo.png` or `icon.png` for screenshots
3. additional README images may also be picked up as gallery content

## License

Add a standard license file if possible.

NukkitHub will try to read license information from:

1. GitHub's license detection
2. a `LICENSE` file in the repository root

If no open-source license is found, the project may appear as All Rights Reserved.

## When A Plugin Will Not Be Indexed

Your plugin may be skipped or removed if any of the following is true:

| Situation                                                                  | Result                   |
| -------------------------------------------------------------------------- | ------------------------ |
| Repository is private                                                      | Not indexed              |
| Repository is archived                                                     | Removed or skipped       |
| Repository is a template                                                   | Skipped                  |
| Repository has `noindex` topic                                             | Skipped or removed       |
| No supported manifest exists in `src/main/resources/`                      | Not discovered correctly |
| The runtime is unclear and there is no matching topic or dependency signal | Module may be skipped    |

To remove your plugin manually, add the `noindex` topic.

## Tips To Get Indexed Faster

1. Add the correct runtime topic immediately
2. Put the manifest in `src/main/resources/` from the start
3. Fill in `name`, `version`, `main`, and `api`
4. Keep README and repository metadata public
5. Publish a GitHub Release if you want downloadable versions visible
6. Add category topics, icon, and screenshots after the plugin is indexed

## Example Repository Structure

```text
my-nukkit-plugin/
├── .github/
│   └── img/
│       ├── logo.png
│       ├── gallery1.png
│       └── gallery2.png
├── src/
│   └── main/
│       ├── java/
│       │   └── com/example/
│       │       └── MyPlugin.java
│       └── resources/
│           └── plugin.yml
├── README.md
└── LICENSE
```

## Troubleshooting

### My plugin is not showing up

Check these items in order:

1. repository is public
2. repository is not archived
3. repository does not have the `noindex` topic
4. manifest exists in `src/main/resources/`
5. the runtime topic is set correctly
6. build files clearly reference the runtime you are targeting
7. wait up to 1 hour for the next indexing cycle

### My project is indexed but has no downloads

Make sure you created a **GitHub Release** and attached a `.jar` file.

### My runtime is detected incorrectly

Use the topic that matches your runtime and make your build files reference the matching core clearly.
