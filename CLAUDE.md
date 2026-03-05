# AllayHub Project Context

## Project Overview
AllayHub is a plugin hub and documentation platform for AllayMC (Minecraft Bedrock Edition server software).

## Key Features
- Gradle plugin indexing and parsing (supports Groovy and KTS DSL)
- Multi-module repository support
- Project cards and author links
- Plugin discovery and documentation

## Technology Stack
- **Frontend**: Nuxt.js (TypeScript)
- **Build Tool**: Bun
- **Styling**: Tailwind CSS
- **Search**: Orama
- **Deployment**: Cloudflare (wrangler)

## Project Structure
- `src/` - Main source code
- `packages/` - Monorepo packages
- `indexer/` - Plugin indexing logic
- `docs/` - Documentation
- `i18n/` - Internationalization files
- `scripts/` - Build and utility scripts

## Development Commands
```bash
# Install dependencies
bun install

# Development server
bun dev

# Build for production
bun build

# Run indexer
bun run indexer
```

## Important Files
- `nuxt.config.ts` - Nuxt configuration
- `package.json` - Dependencies and scripts
- `tailwind.config.ts` - Tailwind CSS configuration
- `orama_builder.mjs` - Search index builder
- `docs/PLUGIN_INDEXING.md` - Plugin indexing documentation

## Code Style
- ESLint configuration: `eslint.config.mjs`
- Prettier ignore: `.prettierignore`
- TypeScript config: `tsconfig.json`

## Notes for AI Assistant
- This is a Nuxt.js project using Bun as the package manager
- Focus on TypeScript best practices
- Follow existing code patterns in the codebase
- Check `docs/PLUGIN_INDEXING.md` for plugin indexing details
