import { transformCrowdinMessages } from '@modrinth/ui'

// eager:false - only loads the locale requested
const localeModules = import.meta.glob('../src/locales/*/index.json', {
  eager: false,
})

export default defineI18nLocale(async (locale) => {
  const loader = localeModules[`../src/locales/${locale}/index.json`]
  if (!loader) {
    console.warn(`Locale ${locale} not found`)
    return {}
  }
  const messages = await loader()
  return transformCrowdinMessages(messages.default)
})
