import { type CrowdinMessages, transformCrowdinMessages } from '@modrinth/ui'

// eager:false - only loads the locale requested
const localeModules = import.meta.glob<{ default: CrowdinMessages }>(
  '../src/locales/*/index.json',
  { eager: false },
)

// Keys defined in @modrinth/ui (e.g. common-messages.ts) live in the
// package's own locale files and must be merged in at load time.
const uiLocaleModules = import.meta.glob<{ default: CrowdinMessages }>(
  '../packages/ui/src/locales/*/index.json',
  { eager: false },
)

export default defineI18nLocale(async (locale) => {
  const loader = localeModules[`../src/locales/${locale}/index.json`]
  const uiLoader =
    uiLocaleModules[`../packages/ui/src/locales/${locale}/index.json`]
  if (!loader && !uiLoader) {
    console.warn(`Locale ${locale} not found`)
    return {}
  }

  const [appModule, uiModule] = await Promise.all([
    loader?.() ?? { default: {} as CrowdinMessages },
    uiLoader?.() ?? { default: {} as CrowdinMessages },
  ])

  // App messages win over UI package messages on key conflicts
  return {
    ...transformCrowdinMessages(uiModule.default),
    ...transformCrowdinMessages(appModule.default),
  }
})
