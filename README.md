# NukkitHub

Plugin hub for the [Nukkit](https://github.com/CloudburstMC/Nukkit), [Nukkit-MOT](https://github.com/MemoriesOfTime/Nukkit-MOT), [PowerNukkitX](https://github.com/PowerNukkitX/PowerNukkitX), and [Lumi](https://github.com/KoshakMineDEV/Lumi) ecosystems.

## Documentation

- [Plugin Indexing Guide](docs/PLUGIN_INDEXING.md) - How Nukkit, Nukkit-MOT, PowerNukkitX, and Lumi plugin repositories are discovered, indexed, and displayed on NukkitHub

## Development

### Requirements

- [Bun](https://bun.sh/) `>= 1.3.6`
- [Rust](https://www.rust-lang.org/tools/install) toolchain

### Run locally

```bash
bun install
bun run dev
```

`bun run dev` will build the search index before starting the Nuxt development server.

### Build `nukkitindexer` manually

```bash
cargo run --manifest-path indexer/Cargo.toml -- discover
cargo run --manifest-path indexer/Cargo.toml -- build
```

## Credits

- Frontend based on [Modrinth](https://github.com/modrinth/code) & [AllayHub](https://github.com/AllayMC/AllayHub)

## License

[AGPL-3.0](LICENSE)
