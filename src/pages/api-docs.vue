<script setup lang="ts">
import { defineMessages, useVIntl } from '@modrinth/ui'

import { computed, useAsyncData, useSeoMeta } from '#imports'

// Renders the repo's docs/API*.md directly — single source of truth, no
// copy under src/assets/docs that could drift out of sync. Locales without
// a translated document fall back to the English one.
const docsModules = import.meta.glob<string>(
  ['../../docs/API.md', '../../docs/API.zh-CN.md'],
  {
    query: '?raw',
    import: 'default',
  },
)

const defaultDocsModule = '../../docs/API.md'
const localizedDocsModules: Record<string, string> = {
  'zh-CN': '../../docs/API.zh-CN.md',
}

async function loadApiDocsMarkdown(localeCode: string): Promise<string> {
  const candidates = [localizedDocsModules[localeCode], defaultDocsModule]
  for (const candidate of candidates) {
    if (!candidate) continue
    const loader = docsModules[candidate]
    if (loader) {
      return loader()
    }
  }
  throw new Error('API document is missing.')
}

const { formatMessage, locale } = useVIntl()

const messages = defineMessages({
  title: {
    id: 'api-docs.title',
    defaultMessage: 'Public API',
  },
  description: {
    id: 'api-docs.description',
    defaultMessage:
      'Read-only JSON API for listing Nukkit-family plugins and resolving installable jar URLs.',
  },
  heroEyebrow: {
    id: 'api-docs.hero-eyebrow',
    defaultMessage: 'For tool developers',
  },
  heroCallout: {
    id: 'api-docs.hero-callout',
    defaultMessage:
      'Two GETs are enough: search/{loader}.json lists the plugins of one runtime, then project/{owner}/{name}/latest.json resolves the installable jar URL.',
  },
  loading: {
    id: 'api-docs.loading',
    defaultMessage: 'Rendering API documentation...',
  },
  renderFailed: {
    id: 'api-docs.render-failed',
    defaultMessage: 'Failed to render the API documentation.',
  },
})

const title = computed(() => formatMessage(messages.title))
const description = computed(() => formatMessage(messages.description))

const docsHtmlCacheKey = computed(() => `api-docs-html:${locale.value}`)

async function renderDocsHtml(): Promise<string> {
  const [{ renderHighlightedString }, docsContent] = await Promise.all([
    import('@modrinth/utils'),
    loadApiDocsMarkdown(locale.value),
  ])

  return renderHighlightedString(docsContent)
}

const { data: renderedDocsHtml, status: renderedDocsStatus } =
  await useAsyncData<string>(docsHtmlCacheKey, renderDocsHtml, {
    default: () => '',
    lazy: true,
    watch: [locale],
  })

const docsHtml = computed(() => renderedDocsHtml.value || '')
const isDocsPending = computed(() =>
  ['idle', 'pending'].includes(renderedDocsStatus.value),
)

useSeoMeta({
  title,
  description,
  ogTitle: title,
  ogDescription: description,
})
</script>

<template>
  <div class="page-container">
    <section class="hero-panel">
      <span class="eyebrow">{{ formatMessage(messages.heroEyebrow) }}</span>
      <h1 class="page-title">{{ formatMessage(messages.title) }}</h1>
      <p class="page-description">{{ formatMessage(messages.description) }}</p>
      <div class="hero-callout">
        {{ formatMessage(messages.heroCallout) }}
      </div>
    </section>

    <section class="docs-card">
      <!-- Rendered from repo-controlled markdown files via highlighted HTML output. -->
      <!-- eslint-disable-next-line vue/no-v-html -->
      <div v-if="docsHtml" class="markdown-body" v-html="docsHtml" />
      <div v-else-if="isDocsPending" class="docs-placeholder">
        {{ formatMessage(messages.loading) }}
      </div>
      <div v-else class="docs-placeholder docs-placeholder--error">
        {{ formatMessage(messages.renderFailed) }}
      </div>
    </section>
  </div>
</template>

<style scoped lang="scss">
.page-container {
  width: calc(100% - 2 * var(--spacing-card-md));
  max-width: 1080px;
  margin-inline: auto;
  box-sizing: border-box;
  margin-block: var(--spacing-card-md);
  display: grid;
  gap: 1.25rem;
}

.hero-panel,
.docs-card {
  border-radius: var(--size-rounded-card);
  border: 1px solid var(--color-divider);
  background: var(--color-raised-bg);
  box-shadow: 0 18px 40px rgb(0 0 0 / 0.08);
}

.hero-panel {
  padding: 1.75rem;
  display: grid;
  gap: 0.9rem;
  background:
    linear-gradient(135deg, rgb(22 163 74 / 0.12), transparent 55%),
    linear-gradient(180deg, var(--color-raised-bg), var(--color-bg));
}

.eyebrow {
  display: inline-flex;
  align-items: center;
  width: fit-content;
  padding: 0.35rem 0.75rem;
  border-radius: 999px;
  font-size: 0.8rem;
  font-weight: 700;
  background: rgb(22 163 74 / 0.12);
  color: rgb(21 128 61);
}

.page-title {
  margin: 0;
  font-size: clamp(2.1rem, 4vw, 3.2rem);
  line-height: 0.98;
  color: var(--color-contrast);
}

.page-description,
.hero-callout {
  margin: 0;
  line-height: 1.65;
  color: var(--color-text-secondary);
}

.hero-callout {
  padding: 0.95rem 1rem;
  border-radius: 1rem;
  background: rgb(255 255 255 / 0.72);
  border: 1px solid var(--color-divider);
}

.docs-card {
  padding: 1.5rem;
}

.docs-placeholder {
  padding: 2rem 1rem;
  border-radius: 1rem;
  border: 1px dashed var(--color-divider);
  text-align: center;
  background: var(--color-bg);
}

.docs-placeholder--error {
  color: var(--color-red);
}

.markdown-body {
  width: 100%;
  max-width: 100%;
  line-height: 1.7;
  color: var(--color-text-secondary);

  :deep(h1),
  :deep(h2),
  :deep(h3),
  :deep(h4),
  :deep(h5),
  :deep(h6) {
    color: var(--color-contrast);
    margin-top: 1.5em;
    margin-bottom: 0.75em;
    font-weight: 600;
    line-height: 1.3;
  }

  :deep(h1) {
    font-size: 2rem;
    border-bottom: 1px solid var(--color-divider);
    padding-bottom: 0.5rem;
  }

  :deep(h2) {
    font-size: 1.75rem;
    border-bottom: 1px solid var(--color-divider);
    padding-bottom: 0.4rem;
  }

  :deep(h3) {
    font-size: 1.35rem;
  }

  :deep(p) {
    margin: 1em 0;
  }

  :deep(a) {
    color: var(--color-brand);
    text-decoration: none;

    &:hover {
      text-decoration: underline;
    }
  }

  :deep(code) {
    background: var(--color-bg);
    padding: 0.2em 0.4em;
    border-radius: var(--radius-sm);
    font-family: var(--font-mono, monospace);
    font-size: 0.9em;
    color: var(--color-contrast);
  }

  :deep(pre) {
    background: var(--color-bg);
    padding: 1rem;
    border-radius: var(--radius-md);
    overflow-x: auto;
    margin: 1.5em 0;

    code {
      background: none;
      padding: 0;
      color: var(--color-text);
      font-size: 0.875rem;
    }
  }

  :deep(blockquote) {
    border-left: 4px solid var(--color-brand);
    margin: 1.5em 0;
    color: var(--color-text-secondary);
    background: var(--color-bg);
    padding: 0.75rem 1rem;
    border-radius: var(--radius-sm);

    p {
      margin: 0.5em 0;
    }
  }

  :deep(ul),
  :deep(ol) {
    margin: 1em 0;
    padding-left: 2em;

    li {
      margin: 0.5em 0;
    }
  }

  :deep(table) {
    width: 100%;
    max-width: 100%;
    display: table;
    table-layout: fixed;
    border-collapse: collapse;
    margin: 1.5em 0;
    background: var(--color-raised-bg);
    border-radius: var(--radius-md);
    overflow: hidden;
    border: 1px solid var(--color-divider);

    th,
    td {
      padding: 0.75rem 1rem;
      text-align: left;
      border-bottom: 1px solid var(--color-divider);
      word-wrap: break-word;
      overflow-wrap: break-word;
      background: var(--color-raised-bg);
    }

    th {
      background: var(--color-button-bg);
      font-weight: 600;
      color: var(--color-contrast);
      border-bottom: 2px solid var(--color-divider);
    }

    tr:last-child td {
      border-bottom: none;
    }
  }
}

@media (max-width: 640px) {
  .page-container {
    width: calc(100% - 1rem);
    gap: 1rem;
  }

  .hero-panel,
  .docs-card {
    padding: 1.1rem;
  }
}
</style>
