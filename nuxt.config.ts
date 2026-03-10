import { existsSync, readdirSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { LOCALES } from '@modrinth/ui/src/composables/i18n.ts'
import serverSidedVue from '@vitejs/plugin-vue'
import { defineNuxtConfig } from 'nuxt/config'
import svgLoader from 'vite-svg-loader'

const favicons = {
  '(prefers-color-scheme:no-preference)': '/favicon.png',
  '(prefers-color-scheme:light)': '/favicon.png',
  '(prefers-color-scheme:dark)': '/favicon.png',
}

export default defineNuxtConfig({
  srcDir: 'src/',
  spaLoadingTemplate: true,
  app: {
    baseURL: '/',
    head: {
      htmlAttrs: {
        lang: 'en',
      },
      title: 'NukkitHub',
      script: [
        {
          innerHTML: `(function(){var w=console.warn,e=console.error;console.warn=function(){if(arguments[0]&&arguments[0].includes&&arguments[0].includes('Hydration'))return;w.apply(console,arguments)};console.error=function(){if(arguments[0]&&arguments[0].includes&&arguments[0].includes('Hydration'))return;e.apply(console,arguments)}})()`,
        },
      ],
      link: [
        ...Object.entries(favicons).map(([media, href]): object => {
          return { rel: 'icon', type: 'image/png', href, media }
        }),
        ...Object.entries(favicons).map(([media, href]): object => {
          return {
            rel: 'apple-touch-icon',
            type: 'image/png',
            href,
            media,
            sizes: '256x256',
          }
        }),
        {
          rel: 'search',
          type: 'application/opensearchdescription+xml',
          href: '/opensearch.xml',
          title: 'NukkitHub plugins',
        },
      ],
    },
  },
  vite: {
    server: {
      fs: {
        allow: ['..'],
      },
    },
    css: {
      preprocessorOptions: {
        scss: {
          // TODO: dont forget about this
          silenceDeprecations: ['import'],
        },
      },
    },
    ssr: {
      // https://github.com/Akryum/floating-vue/issues/809#issuecomment-1002996240
      noExternal: ['v-tooltip', 'xss', 'intl-messageformat'],
    },
    define: {
      global: {},
    },
    esbuild: {
      define: {
        global: 'globalThis',
      },
    },
    cacheDir: '../../node_modules/.vite/apps/knossos',
    resolve: {
      dedupe: ['vue'],
    },
    plugins: [
      svgLoader({
        svgoConfig: {
          plugins: [
            {
              name: 'preset-default',
              params: {
                overrides: {
                  removeViewBox: false,
                },
              },
            },
          ],
        },
      }),
    ],
    build: {
      rollupOptions: {
        output: {
          manualChunks(id) {
            if (id.includes('node_modules')) {
              if (
                id.includes('intl-messageformat') ||
                id.includes('@formatjs')
              ) {
                return 'vendor-i18n'
              }
              if (id.includes('highlight.js')) {
                return 'vendor-highlight'
              }
              if (id.includes('markdown-it')) {
                return 'vendor-markdown'
              }
              if (id.includes('@orama')) {
                return 'vendor-search'
              }
              if (id.includes('floating-vue')) {
                return 'vendor-floating'
              }
            }
          },
        },
      },
    },
  },
  // SSG mode configuration
  nitro: {
    // Static site generation preset
    preset: 'static',
    // Prerender configuration
    prerender: {
      // Crawl all links from starting routes
      crawlLinks: false,
      // Starting routes for crawling
      routes: getPrerenderRoutes(),
      // Ignore routes that intentionally throw 404
      ignore: ['/discover'],
      // Ignore errors for dynamic routes that can't be pre-rendered
      failOnError: true,
    },
    rollupConfig: {
      plugins: [serverSidedVue()],
    },
    hooks: {
      'prerender:generate': (route) => {
        if (route.error) {
          console.error('Prerender error for', route.route)
          console.error('Error:', route.error)
          if (route.error.stack) {
            console.error('Stack:', route.error.stack)
          }
        }
      },
    },
  },
  runtimeConfig: {
    public: {
      siteUrl: getDomain(),
      production: isProduction(),
      featureFlagOverrides: getFeatureFlagOverrides(),

      owner: process.env.VERCEL_GIT_REPO_OWNER || 'MemoriesOfTime',
      slug: process.env.VERCEL_GIT_REPO_SLUG || 'NukkitHub',
      branch:
        process.env.VERCEL_GIT_COMMIT_REF ||
        process.env.CF_PAGES_BRANCH ||
        // @ts-ignore
        globalThis.CF_PAGES_BRANCH ||
        'main',
      hash:
        process.env.VERCEL_GIT_COMMIT_SHA ||
        process.env.CF_PAGES_COMMIT_SHA ||
        // @ts-ignore
        globalThis.CF_PAGES_COMMIT_SHA ||
        'unknown',
    },
  },
  typescript: {
    shim: false,
    strict: true,
    typeCheck: false,
    tsConfig: {
      compilerOptions: {
        moduleResolution: 'bundler',
        allowImportingTsExtensions: true,
      },
    },
  },
  modules: ['@nuxtjs/i18n', '@pinia/nuxt', 'floating-vue/nuxt'],
  // @ts-ignore floating-vue module config
  floatingVue: {
    themes: {
      'ribbit-popout': {
        $extend: 'dropdown',
        placement: 'bottom-end',
        instantMove: true,
        distance: 8,
      },
      'dismissable-prompt': {
        $extend: 'dropdown',
        placement: 'bottom-start',
      },
    },
  },
  i18n: {
    defaultLocale: 'en-US',
    lazy: true,
    langDir: '.',
    locales: LOCALES.map((locale) => ({
      ...locale,
      file: 'locale-loader.js',
    })),
    strategy: 'no_prefix',
    detectBrowserLanguage: {
      useCookie: true,
      cookieKey: 'locale',
      fallbackLocale: 'en-US',
    },
    vueI18n: './i18n.config.js',
    bundle: {
      optimizeTranslationDirective: false,
    },
  },
  devtools: {
    enabled: true,
  },
  css: ['~/assets/styles/tailwind.css'],
  postcss: {
    plugins: {
      tailwindcss: {},
      autoprefixer: {},
    },
  },
  routeRules: {
    '/**': {
      headers: {
        'Accept-CH': 'Sec-CH-Prefers-Color-Scheme',
        'Critical-CH': 'Sec-CH-Prefers-Color-Scheme',
      },
    },
  },
  compatibilityDate: '2025-01-01',
  telemetry: false
})

function isProduction() {
  return process.env.NODE_ENV === 'production'
}

function getFeatureFlagOverrides() {
  return JSON.parse(process.env.FLAG_OVERRIDES ?? '{}')
}

function getDomain() {
  if (process.env.NODE_ENV !== 'production') {
    const port = process.env.PORT || 3000
    return `http://localhost:${port}`
  }
  return 'https://plugins.nukkit-mot.com'
}

function getPrerenderRoutes() {
  const routes = new Set<string>([
    '/',
    '/plugins',
    '/discover/plugins',
    '/flags',
    '/settings',
    '/settings/language',
    '/indexing-guide',
  ])
  const indexDir = 'NukkitHubIndex'

  if (!existsSync(indexDir)) {
    return [...routes]
  }

  const stack = [indexDir]

  while (stack.length > 0) {
    const currentDir = stack.pop()
    if (!currentDir) continue

    for (const entry of readdirSync(currentDir, { withFileTypes: true })) {
      const fullPath = join(currentDir, entry.name)

      if (entry.isDirectory()) {
        stack.push(fullPath)
        continue
      }

      if (!entry.isFile() || !entry.name.endsWith('.json')) {
        continue
      }

      const routePath = relative(indexDir, fullPath)
        .split(sep)
        .join('/')
        .replace(/\.json$/, '')
      const [owner, ...repoParts] = routePath.split('/')

      if (!owner || repoParts.length === 0) {
        continue
      }

      routes.add(`/plugin/${owner}/${repoParts.join('/')}`)
    }
  }

  return [...routes].sort()
}
