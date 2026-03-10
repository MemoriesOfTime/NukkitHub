import {API_VERSIONS, CATEGORIES, PLUGIN_TARGETS} from '~/types/allayhub'

interface StaticCategory {
  id: string
  name: string
  description: string
  icon?: string
  project_type?: 'plugin'
}

const categoryIcons = import.meta.glob<string>(
  '~/assets/images/category/*.svg',
  {
    query: '?raw',
    import: 'default',
    eager: true,
  },
)

function getCategoryIcon(categoryId: string): string | undefined {
  const key = Object.keys(categoryIcons).find((iconPath) =>
    iconPath.includes(`/${categoryId}.svg`),
  )

  return key ? categoryIcons[key] : undefined
}

const categories: StaticCategory[] = CATEGORIES.map((category) => ({
  ...category,
  icon: getCategoryIcon(category.id),
  project_type: 'plugin',
}))

const pluginTargets = PLUGIN_TARGETS.map((target) => ({ ...target }))

export function useCategories() {
  return { data: ref({ categories }) }
}

export function useApiVersions() {
  return { data: ref({ versions: API_VERSIONS }) }
}

export function usePluginTargets() {
  return { data: ref({ targets: pluginTargets }) }
}
