<script setup lang="ts">
import {
  Admonition,
  commonSettingsMessages,
  LanguageSelector,
  languageSelectorMessages,
  LOCALES,
  useVIntl,
} from '@modrinth/ui'

const { formatMessage } = useVIntl()
const { locale, setLocale } = useI18n()

const platform = formatMessage(languageSelectorMessages.platformSite)

const $isChanging = ref(false)

async function onLocaleChange(newLocale: string) {
  if (locale.value === newLocale) return

  $isChanging.value = true
  try {
    // Cast to locale type since the LanguageSelector component provides valid locales
    await setLocale(newLocale as typeof locale.value)
  } finally {
    $isChanging.value = false
  }
}
</script>

<template>
  <div>
    <section class="universal-card">
      <h2 class="text-2xl">
        {{ formatMessage(commonSettingsMessages.language) }}
      </h2>

      <Admonition type="warning">
        {{
          formatMessage(languageSelectorMessages.languageWarning, { platform })
        }}
      </Admonition>

      <ClientOnly>
        <LanguageSelector
          :current-locale="locale"
          :locales="LOCALES"
          :on-locale-change="onLocaleChange"
          :is-changing="$isChanging"
        />
      </ClientOnly>
    </section>
  </div>
</template>
