import type { PluginDocument } from '~/composables/orama-loader'
import type AllayIndex from '~/types/allayhub-index'

export function isTemplatePlaceholder(value: unknown): value is string {
  if (typeof value !== 'string') return false
  const text = value.trim()
  return /^\$\{[^}]+\}$/.test(text) || /^@[^@\r\n]+@$/.test(text)
}

export function getRepoNameFromId(id: string): string {
  const [, repo] = id.split('/')
  return repo || id
}

export function toPluginSummary(doc: PluginDocument): AllayIndex.PluginSummary {
  const displayName = doc.display_name?.trim() || ''
  const fallbackName = getRepoNameFromId(doc.id)
  const safeName =
    displayName && !isTemplatePlaceholder(displayName)
      ? displayName
      : fallbackName

  const summary = doc.summary?.trim() || ''
  const safeSummary = isTemplatePlaceholder(summary) ? '' : summary

  return {
    id: doc.id,
    name: safeName,
    summary: safeSummary,
    author: doc.author,
    categories: doc.categories,
    targets: doc.targets,
    primary_target: doc.primary_target,
    api_version: doc.api_version,
    license: doc.license,
    downloads: doc.downloads,
    stars: doc.stars,
    created_at: new Date(doc.created_at * 1000).toISOString(),
    updated_at: new Date(doc.updated_at * 1000).toISOString(),
    icon_url: doc.icon_url || undefined,
    gallery_image: doc.gallery_image || undefined,
  }
}
