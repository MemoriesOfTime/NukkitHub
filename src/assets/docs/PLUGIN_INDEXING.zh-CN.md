# 插件收录指南

本指南面向插件开发者。如果你希望自己的项目出现在 NukkitHub 中，请按下方清单逐项检查。

## 快速收录清单

要让插件被收录，请确保仓库满足以下全部条件：

1. **使用公开的 GitHub 仓库**
2. **不要归档仓库**
3. **不要将仓库标记为模板**
4. **不要添加 `noindex` topic**
5. **将清单文件放在标准路径中**
   - `src/main/resources/plugin.yml`
   - 对于以 PowerNukkitX 为主的模块，可使用 `src/main/resources/powernukkitx.yml`
6. **添加与你目标运行时匹配的 topic**
7. **完善 `plugin.yml` / `powernukkitx.yml` 与 `README.md`**

如果你除了清单文件之外只打算多做一件事，那就先加上正确的 topic。这样能显著提高被发现的可靠性。

## 应该添加哪个 Topic？

请使用最符合你插件目标运行时的 topic：

| 目标运行时   | 推荐 topic                            |
| ------------ | ------------------------------------- |
| NukkitX      | `nukkit-plugin`                       |
| Nukkit-MOT   | `nukkit-mot-plugin`                   |
| PowerNukkitX | `powernukkitx-plugin` 或 `pnx-plugin` |
| Lumi         | `lumi-plugin`                         |

如果你的插件同时支持 NukkitX 与 Nukkit-MOT，使用 `nukkit-plugin` 也是可以的。

**如何添加 topic：** GitHub 仓库 -> About -> 齿轮图标 -> Topics。

> **Fork 说明：** 如果你的插件仓库是 fork，添加正确的 topic 会更加重要。

## 各运行时应该怎么做

### NukkitX (`nkx`)

推荐配置：

- 使用 `src/main/resources/plugin.yml`
- 添加 `nukkit-plugin` topic
- 在仓库或构建文件中引用 NukkitX 相关的运行时依赖或强标识，例如：
  - `cloudburstmc`
  - `repo.nukkitx.com`

如果你的插件使用通用的 `cn.nukkit:*` 依赖，它可能会被识别为同时兼容 NukkitX 和 Nukkit-MOT。

### Nukkit-MOT (`nkmot`)

推荐配置：

- 使用 `src/main/resources/plugin.yml`
- 添加 `nukkit-mot-plugin` topic
- 在构建文件中引用 Nukkit-MOT 相关名称，例如：
  - `memoriesoftime`
  - `nukkit-mot`

如果你的插件同时支持 NukkitX 与 Nukkit-MOT，通用的 `cn.nukkit:*` 依赖也是可接受的。

> **重要：** MOTCI 不是仓库提交渠道。你的插件仍然需要能从 GitHub 被正常发现。

### PowerNukkitX (`pnx`)

推荐配置：

- 优先使用 `src/main/resources/powernukkitx.yml`
- 添加 `powernukkitx-plugin` 或 `pnx-plugin`
- 引用 PowerNukkitX 相关依赖，例如：
  - `cn.powernukkitx`
  - `powernukkitx`

使用 `powernukkitx.yml` 是让 PowerNukkitX 模块被正确识别的最明确方式。

### Lumi (`lumi`)

推荐配置：

- 使用 `src/main/resources/plugin.yml`
- 添加 `lumi-plugin` topic
- 在 Gradle 或 Maven 中显式引用 Lumi，例如：
  - `repo.luminiadev.com`
  - `com.koshakmine:lumi`
  - 包含 `com.koshakmine` 与 `lumi` 的 Maven 坐标

## 清单文件要求

至少需要在标准路径中存在一个受支持的清单文件：

- `src/main/resources/plugin.yml`
- `src/main/resources/powernukkitx.yml`

示例：

```yaml
name: MyNukkitPlugin
version: 1.0.0
main: com.example.MyNukkitPlugin
api: ['1.0.0']
description: A short description of my plugin
website: https://example.com
depend: []
softdepend: []
```

### 必填元数据

| 字段          | 必填 | 说明            |
| ------------- | ---- | --------------- |
| `name`        | 是   | 插件名称        |
| `version`     | 是   | 插件版本        |
| `main`        | 是   | 主类路径        |
| `api`         | 是   | 支持的 API 版本 |
| `description` | 否   | 简短说明        |
| `website`     | 否   | 主页链接        |
| `depend`      | 否   | 必需依赖        |
| `softdepend`  | 否   | 可选依赖        |

作者信息会优先从 GitHub contributors 中索引；如果没有可用贡献者，则使用仓库所有者。

## 多模块仓库

如果你的仓库包含多个插件模块：

- 每个模块都应在 `src/main/resources/` 下拥有自己的清单文件
- 每个模块会被分别收录
- 生成的 ID 形式类似 `owner/repo--module-suffix`

示例：

- 单模块仓库：`cooldev/myplugin-repo`
- 多模块仓库：`cooldev/myplugin-repo--modules-economy`

## 版本与下载

你的仓库即使没有 GitHub Release，也可以被收录。

但如果你希望用户在 NukkitHub 上看到可下载的版本，请按如下方式发布：

1. 创建 GitHub Release
2. 附加一个或多个 `.jar` 文件
3. 使用 Release 标题和正文作为版本标题与更新日志

只有 **GitHub Releases** 会被索引为插件版本。

## 分类

添加分类 topic 可以帮助用户更快找到你的插件：

| 分类             | Topic                     |
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

## README、图标与截图

为了让你的页面在收录后看起来更完整：

### README

你的 `README.md` 会作为主要的长描述内容。

建议：

- 说明插件具体做什么
- 展示使用截图
- 包含安装步骤
- 保持所有链接有效且公开可访问

### 图标

如需添加自定义图标：

1. 将 `logo.png` 或 `icon.png` 放在 `.github/img/` 或仓库根目录
2. 使用 PNG 格式
3. 推荐尺寸：`256x256`

如果未找到图标，将使用仓库所有者头像作为默认图标。

### 画廊截图

如需添加截图：

1. 将 PNG 文件放在 `.github/img/`
2. 不要把 `logo.png` 或 `icon.png` 当作截图
3. README 中的额外图片也可能被收集为画廊内容

## 许可证

如果可以，请添加标准许可证文件。

NukkitHub 会尝试从以下位置读取许可证信息：

1. GitHub 的许可证识别结果
2. 仓库根目录中的 `LICENSE` 文件

如果未找到开源许可证，项目可能会显示为 All Rights Reserved。

## 插件在什么情况下不会被收录

如果满足以下任一情况，插件可能会被跳过或移除：

| 情况                                           | 结果           |
| ---------------------------------------------- | -------------- |
| 仓库为私有                                     | 不会被收录     |
| 仓库已归档                                     | 被移除或跳过   |
| 仓库是模板                                     | 被跳过         |
| 仓库带有 `noindex` topic                       | 被跳过或移除   |
| `src/main/resources/` 下不存在受支持的清单文件 | 无法被正确发现 |
| 运行时不明确，且没有匹配的 topic 或依赖信号    | 模块可能被跳过 |

如果你想手动移除插件，请添加 `noindex` topic。

## 更快被收录的小提示

1. 立即添加正确的运行时 topic
2. 从一开始就把清单文件放在 `src/main/resources/`
3. 完整填写 `name`、`version`、`main` 和 `api`
4. 保持 README 与仓库元数据公开可见
5. 如果希望展示可下载版本，请发布 GitHub Release
6. 在插件被收录后，再补充分类型 topic、图标和截图

## 仓库结构示例

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

## 故障排查

### 我的插件没有显示出来

请按顺序检查以下项目：

1. 仓库是否为公开状态
2. 仓库是否未归档
3. 仓库是否没有 `noindex` topic
4. 清单文件是否存在于 `src/main/resources/`
5. 运行时 topic 是否设置正确
6. 构建文件是否清晰引用了目标运行时
7. 等待最多 1 小时以完成下一轮索引

### 我的项目已被收录，但没有下载项

请确认你已创建 **GitHub Release**，并附加了 `.jar` 文件。

### 我的运行时识别错误

请使用与你的运行时匹配的 topic，并在构建文件中清晰引用对应核心实现。
