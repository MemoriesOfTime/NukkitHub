# AllayHub

Plugin hub for [Nukkit](https://github.com/CloudburstMC/Nukkit) - Minecraft Bedrock Edition server software.

## Features

- **Automatic Plugin Discovery** - Indexes Nukkit plugins from GitHub repositories
- **Version Management** - Tracks plugin releases and versions
- **Search & Filter** - Find plugins by category, API version, and more
- **Rich Metadata** - Displays plugin descriptions, screenshots, and documentation

## Documentation

- [Plugin Indexing Guide](docs/PLUGIN_INDEXING.md) - How to get your plugin indexed on AllayHub

## For Plugin Developers

To get your Nukkit plugin indexed:

1. Ensure your repository has `plugin.yml` in `src/main/resources/`
2. Add the `nukkit-plugin` topic to your repository
3. Create GitHub releases with your plugin JAR files
4. Wait for the next indexing cycle (runs hourly)

See the [Plugin Indexing Guide](docs/PLUGIN_INDEXING.md) for detailed instructions.

## Project Structure

- `indexer/` - Rust-based plugin indexer that discovers and processes plugins
- `src/` - Vue.js frontend for browsing and searching plugins
- `docs/` - Documentation

## Development

### Indexer

```bash
cd indexer
cargo build --release
cargo run -- discover
```

### Frontend

```bash
bun install
bun dev
```

## License

See LICENSE file for details.
