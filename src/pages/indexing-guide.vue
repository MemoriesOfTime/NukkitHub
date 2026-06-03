<script setup lang="ts">
import { computed, ref, useAsyncData, useSeoMeta } from '#imports'

import {
  type IndexingGuideCoreId,
  loadIndexingGuideMarkdown,
  useIndexingGuideContent
} from '../composables/useIndexingGuideContent'

const {
  locale,
  formatMessage,
  messages,
  title,
  description,
  checklistItems,
  coreRecipes,
  pitfallItems,
} = useIndexingGuideContent()

const activeCoreId = ref<IndexingGuideCoreId>('nkx')
const activeCoreRecipe = computed(
  () =>
    coreRecipes.value.find((core) => core.id === activeCoreId.value) ??
    coreRecipes.value[0],
)
const guideHtmlCacheKey = computed(() => `indexing-guide-html:${locale.value}`)

async function renderGuideHtml(): Promise<string> {
  const [{ renderHighlightedString }, indexingGuideContent] = await Promise.all(
    [import('@modrinth/utils'), loadIndexingGuideMarkdown(locale.value)],
  )

  return renderHighlightedString(indexingGuideContent)
}

const { data: renderedGuideHtml, status: renderedGuideStatus } =
  await useAsyncData<string>(guideHtmlCacheKey, renderGuideHtml, {
    default: () => '',
    lazy: true,
    watch: [locale],
  })

const guideHtml = computed(() => renderedGuideHtml.value || '')
const isGuidePending = computed(() =>
  ['idle', 'pending'].includes(renderedGuideStatus.value),
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

    <section class="guide-section">
      <div class="section-heading">
        <span class="section-kicker">{{
          formatMessage(messages.checklistKicker)
        }}</span>
        <h2>{{ formatMessage(messages.checklistHeading) }}</h2>
        <p>{{ formatMessage(messages.checklistDescription) }}</p>
      </div>
      <div class="checklist-grid">
        <article
          v-for="item in checklistItems"
          :key="item.id"
          class="checklist-card"
        >
          <div class="card-header checklist-card-header">
            <h3>{{ item.title }}</h3>
            <span class="badge">{{ item.badge }}</span>
          </div>
          <p>{{ item.description }}</p>
        </article>
      </div>
    </section>

    <section class="guide-section">
      <div class="section-heading">
        <span class="section-kicker">{{
          formatMessage(messages.coreKicker)
        }}</span>
        <h2>{{ formatMessage(messages.coreHeading) }}</h2>
        <p>{{ formatMessage(messages.coreDescription) }}</p>
      </div>
      <div
        class="core-switcher"
        role="tablist"
        :aria-label="formatMessage(messages.coreSelectorAriaLabel)"
      >
        <button
          v-for="core in coreRecipes"
          :key="core.id"
          type="button"
          class="core-switcher-button"
          :class="{ active: core.id === activeCoreId }"
          :aria-pressed="core.id === activeCoreId"
          @click="activeCoreId = core.id"
        >
          {{ core.title }}
        </button>
      </div>
      <div class="core-grid">
        <article v-if="activeCoreRecipe" class="core-card">
          <div class="card-header">
            <h3>{{ activeCoreRecipe.title }}</h3>
            <span class="core-label">{{ activeCoreRecipe.label }}</span>
          </div>
          <div class="core-meta">
            <div>
              <span class="meta-label">{{
                formatMessage(messages.coreManifestLabel)
              }}</span>
              <p>{{ activeCoreRecipe.manifest }}</p>
            </div>
            <div>
              <span class="meta-label">{{
                formatMessage(messages.coreTopicsLabel)
              }}</span>
              <p>{{ activeCoreRecipe.topics.join(', ') }}</p>
            </div>
          </div>
          <ul class="action-list">
            <li v-for="action in activeCoreRecipe.actions" :key="action">
              {{ action }}
            </li>
          </ul>
          <p class="core-note">{{ activeCoreRecipe.note }}</p>
        </article>
      </div>
    </section>

    <section class="guide-section">
      <div class="section-heading">
        <span class="section-kicker">{{
          formatMessage(messages.pitfallsKicker)
        }}</span>
        <h2>{{ formatMessage(messages.pitfallsHeading) }}</h2>
      </div>
      <div class="pitfall-grid">
        <article
          v-for="item in pitfallItems"
          :key="item.id"
          class="pitfall-card"
        >
          <h3>{{ item.title }}</h3>
          <p>{{ item.description }}</p>
        </article>
      </div>
    </section>

    <section class="guide-card">
      <div class="guide-card-header">
        <div>
          <span class="section-kicker">{{
            formatMessage(messages.fullGuideKicker)
          }}</span>
          <h2>{{ formatMessage(messages.fullGuideHeading) }}</h2>
        </div>
        <p>{{ formatMessage(messages.fullGuideDescription) }}</p>
      </div>

      <!-- Rendered from repo-controlled markdown files via highlighted HTML output. -->
      <!-- eslint-disable-next-line vue/no-v-html -->
      <div v-if="guideHtml" class="markdown-body" v-html="guideHtml" />
      <div v-else-if="isGuidePending" class="guide-placeholder">
        {{ formatMessage(messages.loading) }}
      </div>
      <div v-else class="guide-placeholder guide-placeholder--error">
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
.guide-section,
.guide-card {
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

.eyebrow,
.section-kicker,
.badge,
.core-label {
  display: inline-flex;
  align-items: center;
  width: fit-content;
  border-radius: 999px;
  font-size: 0.8rem;
  font-weight: 700;
}

.eyebrow,
.section-kicker {
  padding: 0.35rem 0.75rem;
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
.hero-callout,
.section-heading p,
.guide-card-header p,
.checklist-card p,
.pitfall-card p,
.core-note,
.markdown-body {
  color: var(--color-text-secondary);
}

.page-description,
.hero-callout,
.section-heading p,
.guide-card-header p,
.checklist-card p,
.pitfall-card p,
.core-note {
  margin: 0;
  line-height: 1.65;
}

.hero-callout {
  padding: 0.95rem 1rem;
  border-radius: 1rem;
  background: rgb(255 255 255 / 0.72);
  border: 1px solid var(--color-divider);
}

.guide-section,
.guide-card {
  padding: 1.5rem;
}

.section-heading,
.guide-card-header {
  display: grid;
  gap: 0.7rem;
  margin-bottom: 1rem;
}

.section-heading h2,
.guide-card-header h2 {
  margin: 0;
  color: var(--color-contrast);
  font-size: clamp(1.45rem, 2.6vw, 2.1rem);
}

.checklist-grid,
.core-grid,
.pitfall-grid {
  display: grid;
  gap: 1rem;
}

.checklist-grid {
  grid-template-columns: 1fr;
  max-width: 58rem;
}

.core-switcher {
  display: flex;
  flex-wrap: wrap;
  gap: 0.75rem;
  margin-bottom: 1rem;
}

.core-switcher-button {
  appearance: none;
  border: 1px solid var(--color-divider);
  background: var(--color-bg);
  color: var(--color-text-secondary);
  border-radius: 999px;
  padding: 0.65rem 0.95rem;
  font: inherit;
  font-weight: 700;
  line-height: 1.2;
  cursor: pointer;
  transition:
    background-color 0.2s ease,
    color 0.2s ease,
    border-color 0.2s ease,
    transform 0.2s ease;

  &:hover {
    border-color: var(--color-brand);
    color: var(--color-contrast);
    transform: translateY(-1px);
  }

  &.active {
    background: var(--color-brand);
    border-color: var(--color-brand);
    color: white;
  }
}

.core-grid {
  grid-template-columns: 1fr;
  max-width: 62rem;
}

.pitfall-grid {
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
}

.checklist-card,
.core-card,
.pitfall-card {
  border-radius: 1rem;
  border: 1px solid var(--color-divider);
  background: rgb(255 255 255 / 0.68);
  padding: 1.1rem;
}

.checklist-card {
  display: grid;
  gap: 0.75rem;
}

.card-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.75rem;
  margin-bottom: 0.7rem;
}

.card-header h3,
.pitfall-card h3 {
  margin: 0;
  color: var(--color-contrast);
}

.checklist-card-header {
  display: grid;
  gap: 0.55rem;
  margin-bottom: 0;
}

.badge {
  padding: 0.3rem 0.65rem;
  background: rgb(37 99 235 / 0.12);
  color: var(--color-brand);
}

.core-label {
  padding: 0.35rem 0.7rem;
  background: var(--color-brand);
  color: white;
}

.core-card {
  display: grid;
  gap: 0.9rem;
}

.core-meta {
  display: grid;
  gap: 0.8rem;
}

.meta-label {
  display: block;
  margin-bottom: 0.25rem;
  font-size: 0.75rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-secondary);
}

.core-meta p {
  margin: 0;
  color: var(--color-contrast);
  line-height: 1.55;
}

.action-list {
  margin: 0;
  padding-left: 1.2rem;
  display: grid;
  gap: 0.4rem;
  color: var(--color-text-secondary);
}

.core-note {
  padding: 0.85rem 0.95rem;
  border-radius: 0.9rem;
  background: var(--color-bg);
  border: 1px dashed var(--color-divider);
}

.guide-placeholder {
  padding: 2rem 1rem;
  border-radius: 1rem;
  border: 1px dashed var(--color-divider);
  text-align: center;
  background: var(--color-bg);
}

.guide-placeholder--error {
  color: var(--color-red);
}

.markdown-body {
  width: 100%;
  max-width: 100%;
  line-height: 1.7;

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
  .guide-section,
  .guide-card {
    padding: 1.1rem;
  }

  .checklist-grid,
  .core-grid,
  .pitfall-grid {
    grid-template-columns: 1fr;
  }
}
</style>
