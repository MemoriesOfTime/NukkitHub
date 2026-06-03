import { defineMessages, type MessageDescriptor, useVIntl } from '@modrinth/ui'
import { computed } from 'vue'

type ChecklistBlueprint = {
  id: string
  title: MessageDescriptor
  badge: MessageDescriptor
  description: MessageDescriptor
}

type CoreRecipeBlueprint = {
  id: 'nkx' | 'nkmot' | 'pnx' | 'lumi'
  label: string
  title: MessageDescriptor
  manifest: MessageDescriptor
  topics: string[]
  actions: MessageDescriptor[]
  note: MessageDescriptor
}

export type IndexingGuideCoreId = CoreRecipeBlueprint['id']

type PitfallBlueprint = {
  id: string
  title: MessageDescriptor
  description: MessageDescriptor
}

const guideMessages = defineMessages({
  requiredBadge: {
    id: 'indexing-guide.badge.required',
    defaultMessage: 'Required',
  },
  recommendedBadge: {
    id: 'indexing-guide.badge.recommended',
    defaultMessage: 'Recommended',
  },
  optionalBadge: {
    id: 'indexing-guide.badge.optional',
    defaultMessage: 'Optional',
  },
  publicRepoTitle: {
    id: 'indexing-guide.checklist.public-repo.title',
    defaultMessage: 'Use a public GitHub repository',
  },
  publicRepoDescription: {
    id: 'indexing-guide.checklist.public-repo.description',
    defaultMessage:
      'Private repositories are not indexed. Keep the repository public and active.',
  },
  manifestPathTitle: {
    id: 'indexing-guide.checklist.manifest-path.title',
    defaultMessage: 'Put your manifest in the standard path',
  },
  manifestPathDescription: {
    id: 'indexing-guide.checklist.manifest-path.description',
    defaultMessage:
      'Use `src/main/resources/plugin.yml`, or `src/main/resources/powernukkitx.yml` for PowerNukkitX-first modules.',
  },
  topicTitle: {
    id: 'indexing-guide.checklist.topic.title',
    defaultMessage: 'Add the right topic for your core',
  },
  topicDescription: {
    id: 'indexing-guide.checklist.topic.description',
    defaultMessage:
      'Topics help your plugin get discovered faster and more reliably than waiting on GitHub code indexing alone.',
  },
  metadataTitle: {
    id: 'indexing-guide.checklist.metadata.title',
    defaultMessage: 'Complete your metadata and README',
  },
  metadataDescription: {
    id: 'indexing-guide.checklist.metadata.description',
    defaultMessage:
      'A clear `plugin.yml`, README, icon, and categories make the listing more complete after inclusion.',
  },
  indexableTitle: {
    id: 'indexing-guide.checklist.indexable.title',
    defaultMessage: 'Keep the repository indexable',
  },
  indexableDescription: {
    id: 'indexing-guide.checklist.indexable.description',
    defaultMessage:
      'Do not archive the repository, do not mark it as a template, and do not add the `noindex` topic.',
  },
  releasesTitle: {
    id: 'indexing-guide.checklist.releases.title',
    defaultMessage: 'Publish GitHub Releases for downloadable versions',
  },
  releasesDescription: {
    id: 'indexing-guide.checklist.releases.description',
    defaultMessage:
      'Your project can be indexed without a release, but only GitHub Releases with `.jar` files become version downloads.',
  },
  nkxTitle: {
    id: 'indexing-guide.core.nkx.title',
    defaultMessage: 'NukkitX plugins',
  },
  nkxManifest: {
    id: 'indexing-guide.core.nkx.manifest',
    defaultMessage: '`src/main/resources/plugin.yml`',
  },
  nkxActionRuntimeMarkers: {
    id: 'indexing-guide.core.nkx.action.1',
    defaultMessage:
      'Use NukkitX-related runtime dependencies or strong repository markers such as `cloudburstmc` or `repo.nukkitx.com`.',
  },
  nkxActionSharedCompatibility: {
    id: 'indexing-guide.core.nkx.action.2',
    defaultMessage:
      'If your plugin is meant for both NukkitX and Nukkit-MOT, generic `cn.nukkit:*` dependencies are acceptable.',
  },
  nkxNote: {
    id: 'indexing-guide.core.nkx.note',
    defaultMessage:
      'If you want shared compatibility with Nukkit-MOT, `nukkit-plugin` plus `cn.nukkit:*` is enough.',
  },
  nkmotTitle: {
    id: 'indexing-guide.core.nkmot.title',
    defaultMessage: 'Nukkit-MOT plugins',
  },
  nkmotManifest: {
    id: 'indexing-guide.core.nkmot.manifest',
    defaultMessage: '`src/main/resources/plugin.yml`',
  },
  nkmotActionRuntimeMarkers: {
    id: 'indexing-guide.core.nkmot.action.1',
    defaultMessage:
      'Use Nukkit-MOT-related dependency names or repository markers such as `memoriesoftime` or `nukkit-mot`.',
  },
  nkmotActionSharedCompatibility: {
    id: 'indexing-guide.core.nkmot.action.2',
    defaultMessage:
      'If the plugin supports both NukkitX and Nukkit-MOT, `cn.nukkit:*` still works as a shared compatibility signal.',
  },
  nkmotNote: {
    id: 'indexing-guide.core.nkmot.note',
    defaultMessage:
      'MOTCI does not submit repositories for indexing. Your plugin still needs to be discoverable from GitHub.',
  },
  pnxTitle: {
    id: 'indexing-guide.core.pnx.title',
    defaultMessage: 'PowerNukkitX plugins',
  },
  pnxManifest: {
    id: 'indexing-guide.core.pnx.manifest',
    defaultMessage: '`src/main/resources/powernukkitx.yml` (preferred)',
  },
  pnxActionDependency: {
    id: 'indexing-guide.core.pnx.action.1',
    defaultMessage:
      'Use PowerNukkitX-related dependencies such as `cn.powernukkitx`.',
  },
  pnxActionPluginYml: {
    id: 'indexing-guide.core.pnx.action.2',
    defaultMessage:
      'If you must keep `plugin.yml`, make sure your build files clearly reference PowerNukkitX so the project is classified correctly.',
  },
  pnxNote: {
    id: 'indexing-guide.core.pnx.note',
    defaultMessage:
      'Using `powernukkitx.yml` is the clearest way to make a PowerNukkitX module indexable.',
  },
  lumiTitle: {
    id: 'indexing-guide.core.lumi.title',
    defaultMessage: 'Lumi plugins',
  },
  lumiManifest: {
    id: 'indexing-guide.core.lumi.manifest',
    defaultMessage: '`src/main/resources/plugin.yml`',
  },
  lumiActionDependency: {
    id: 'indexing-guide.core.lumi.action.1',
    defaultMessage:
      'Reference Lumi in Gradle or Maven, for example `com.koshakmine:lumi`.',
  },
  lumiActionMaven: {
    id: 'indexing-guide.core.lumi.action.2',
    defaultMessage:
      'If you use Maven, keep `com.koshakmine` and `lumi` visible in `pom.xml` so the target can be recognized.',
  },
  lumiNote: {
    id: 'indexing-guide.core.lumi.note',
    defaultMessage:
      'For Lumi projects, the topic plus explicit Lumi dependency coordinates is the safest combination.',
  },
  manifestLocationTitle: {
    id: 'indexing-guide.pitfall.manifest-location.title',
    defaultMessage: 'Wrong manifest location',
  },
  manifestLocationDescription: {
    id: 'indexing-guide.pitfall.manifest-location.description',
    defaultMessage:
      'If the manifest is not under `src/main/resources/`, the repository is very easy to miss.',
  },
  missingSignalTitle: {
    id: 'indexing-guide.pitfall.missing-signal.title',
    defaultMessage: 'No matching topic or core signal',
  },
  missingSignalDescription: {
    id: 'indexing-guide.pitfall.missing-signal.description',
    defaultMessage:
      'A manifest alone is not always enough for runtime classification. Add the topic that matches your target core.',
  },
  repositoryStateTitle: {
    id: 'indexing-guide.pitfall.repository-state.title',
    defaultMessage: 'Archived, template, or `noindex` repository',
  },
  repositoryStateDescription: {
    id: 'indexing-guide.pitfall.repository-state.description',
    defaultMessage:
      'Any of these states will stop the project from being indexed or keep it out of the index.',
  },
  motciTitle: {
    id: 'indexing-guide.pitfall.motci.title',
    defaultMessage: 'Relying on MOTCI alone',
  },
  motciDescription: {
    id: 'indexing-guide.pitfall.motci.description',
    defaultMessage:
      'MOTCI is not a discovery source. Developers still need a valid GitHub repository layout and GitHub topics.',
  },
  title: {
    id: 'indexing-guide.developer-title',
    defaultMessage: 'Get Your Plugin Indexed',
  },
  description: {
    id: 'indexing-guide.developer-description',
    defaultMessage:
      'A practical checklist for plugin developers who want their projects included on NukkitHub.',
  },
  loading: {
    id: 'indexing-guide.loading',
    defaultMessage: 'Rendering indexing guide...',
  },
  renderFailed: {
    id: 'indexing-guide.render-failed',
    defaultMessage: 'Failed to render the indexing guide content.',
  },
  heroEyebrow: {
    id: 'indexing-guide.hero-eyebrow',
    defaultMessage: 'For plugin developers',
  },
  heroCallout: {
    id: 'indexing-guide.hero-callout',
    defaultMessage:
      'The fastest path is simple: public GitHub repo, correct manifest path, matching topic, and a runtime-specific dependency setup.',
  },
  checklistKicker: {
    id: 'indexing-guide.section.checklist.kicker',
    defaultMessage: 'Quick checklist',
  },
  checklistHeading: {
    id: 'indexing-guide.section.checklist.heading',
    defaultMessage: 'Do these first',
  },
  checklistDescription: {
    id: 'indexing-guide.section.checklist.description',
    defaultMessage:
      'If your repository matches this checklist, it has a good chance of being included without any extra manual handling.',
  },
  coreKicker: {
    id: 'indexing-guide.section.core.kicker',
    defaultMessage: 'Choose your core',
  },
  coreHeading: {
    id: 'indexing-guide.section.core.heading',
    defaultMessage: 'What to add for each runtime',
  },
  coreDescription: {
    id: 'indexing-guide.section.core.description',
    defaultMessage:
      'Pick the card that matches your target runtime and follow that recipe. You do not need to understand the detection internals to get indexed.',
  },
  coreSelectorAriaLabel: {
    id: 'indexing-guide.section.core.selector-aria',
    defaultMessage: 'Runtime selector',
  },
  coreManifestLabel: {
    id: 'indexing-guide.section.core.meta.manifest',
    defaultMessage: 'Manifest',
  },
  coreTopicsLabel: {
    id: 'indexing-guide.section.core.meta.topics',
    defaultMessage: 'Topics',
  },
  pitfallsKicker: {
    id: 'indexing-guide.section.pitfalls.kicker',
    defaultMessage: 'Avoid these mistakes',
  },
  pitfallsHeading: {
    id: 'indexing-guide.section.pitfalls.heading',
    defaultMessage: 'Common reasons a plugin is not included',
  },
  fullGuideKicker: {
    id: 'indexing-guide.section.full-guide.kicker',
    defaultMessage: 'Full guide',
  },
  fullGuideHeading: {
    id: 'indexing-guide.section.full-guide.heading',
    defaultMessage: 'Detailed checklist and examples',
  },
  fullGuideDescription: {
    id: 'indexing-guide.section.full-guide.description',
    defaultMessage:
      'The full document below keeps all of the practical details in one place, including manifest examples, versions, icons, and troubleshooting.',
  },
})

const checklistBlueprint: ChecklistBlueprint[] = [
  {
    id: 'public-repo',
    title: guideMessages.publicRepoTitle,
    badge: guideMessages.requiredBadge,
    description: guideMessages.publicRepoDescription,
  },
  {
    id: 'manifest-path',
    title: guideMessages.manifestPathTitle,
    badge: guideMessages.requiredBadge,
    description: guideMessages.manifestPathDescription,
  },
  {
    id: 'topic',
    title: guideMessages.topicTitle,
    badge: guideMessages.recommendedBadge,
    description: guideMessages.topicDescription,
  },
  {
    id: 'metadata',
    title: guideMessages.metadataTitle,
    badge: guideMessages.recommendedBadge,
    description: guideMessages.metadataDescription,
  },
  {
    id: 'indexable',
    title: guideMessages.indexableTitle,
    badge: guideMessages.requiredBadge,
    description: guideMessages.indexableDescription,
  },
  {
    id: 'releases',
    title: guideMessages.releasesTitle,
    badge: guideMessages.optionalBadge,
    description: guideMessages.releasesDescription,
  },
]

const coreRecipeBlueprint: CoreRecipeBlueprint[] = [
  {
    id: 'nkx',
    label: 'NKX',
    title: guideMessages.nkxTitle,
    manifest: guideMessages.nkxManifest,
    topics: ['`nukkit-plugin`'],
    actions: [
      guideMessages.nkxActionRuntimeMarkers,
      guideMessages.nkxActionSharedCompatibility,
    ],
    note: guideMessages.nkxNote,
  },
  {
    id: 'nkmot',
    label: 'NKMOT',
    title: guideMessages.nkmotTitle,
    manifest: guideMessages.nkmotManifest,
    topics: ['`nukkit-mot-plugin`', '`nukkit-plugin` for shared support'],
    actions: [
      guideMessages.nkmotActionRuntimeMarkers,
      guideMessages.nkmotActionSharedCompatibility,
    ],
    note: guideMessages.nkmotNote,
  },
  {
    id: 'pnx',
    label: 'PNX',
    title: guideMessages.pnxTitle,
    manifest: guideMessages.pnxManifest,
    topics: ['`powernukkitx-plugin`', '`pnx-plugin`'],
    actions: [
      guideMessages.pnxActionDependency,
      guideMessages.pnxActionPluginYml,
    ],
    note: guideMessages.pnxNote,
  },
  {
    id: 'lumi',
    label: 'LUMI',
    title: guideMessages.lumiTitle,
    manifest: guideMessages.lumiManifest,
    topics: ['`lumi-plugin`'],
    actions: [
      guideMessages.lumiActionDependency,
      guideMessages.lumiActionMaven,
    ],
    note: guideMessages.lumiNote,
  },
]

const pitfallBlueprint: PitfallBlueprint[] = [
  {
    id: 'manifest-location',
    title: guideMessages.manifestLocationTitle,
    description: guideMessages.manifestLocationDescription,
  },
  {
    id: 'missing-signal',
    title: guideMessages.missingSignalTitle,
    description: guideMessages.missingSignalDescription,
  },
  {
    id: 'repository-state',
    title: guideMessages.repositoryStateTitle,
    description: guideMessages.repositoryStateDescription,
  },
  {
    id: 'motci',
    title: guideMessages.motciTitle,
    description: guideMessages.motciDescription,
  },
]

const guideDocumentModules = import.meta.glob<string>(
  '../assets/docs/PLUGIN_INDEXING*.md',
  {
    query: '?raw',
    import: 'default',
  },
)

const defaultGuideDocument = '../assets/docs/PLUGIN_INDEXING.md'
const localizedGuideDocuments: Record<string, string> = {
  'zh-CN': '../assets/docs/PLUGIN_INDEXING.zh-CN.md',
}

function getGuideDocumentCandidates(localeCode: string): string[] {
  const localizedGuideDocument = localizedGuideDocuments[localeCode]
  return localizedGuideDocument
    ? [localizedGuideDocument, defaultGuideDocument]
    : [defaultGuideDocument]
}

export async function loadIndexingGuideMarkdown(
  localeCode: string,
): Promise<string> {
  for (const candidate of getGuideDocumentCandidates(localeCode)) {
    const loader = guideDocumentModules[candidate]
    if (loader) {
      return loader()
    }
  }

  const fallbackLoader = guideDocumentModules[defaultGuideDocument]
  if (!fallbackLoader) {
    throw new Error('Default indexing guide document is missing.')
  }

  return fallbackLoader()
}

export function useIndexingGuideContent() {
  const { formatMessage, locale } = useVIntl()

  function formatForLocale(
    descriptor: MessageDescriptor,
    currentLocale: string,
  ): string {
    return currentLocale ? formatMessage(descriptor) : formatMessage(descriptor)
  }

  const messages = {
    title: guideMessages.title,
    description: guideMessages.description,
    loading: guideMessages.loading,
    renderFailed: guideMessages.renderFailed,
    heroEyebrow: guideMessages.heroEyebrow,
    heroCallout: guideMessages.heroCallout,
    checklistKicker: guideMessages.checklistKicker,
    checklistHeading: guideMessages.checklistHeading,
    checklistDescription: guideMessages.checklistDescription,
    coreKicker: guideMessages.coreKicker,
    coreHeading: guideMessages.coreHeading,
    coreDescription: guideMessages.coreDescription,
    coreSelectorAriaLabel: guideMessages.coreSelectorAriaLabel,
    coreManifestLabel: guideMessages.coreManifestLabel,
    coreTopicsLabel: guideMessages.coreTopicsLabel,
    pitfallsKicker: guideMessages.pitfallsKicker,
    pitfallsHeading: guideMessages.pitfallsHeading,
    fullGuideKicker: guideMessages.fullGuideKicker,
    fullGuideHeading: guideMessages.fullGuideHeading,
    fullGuideDescription: guideMessages.fullGuideDescription,
  }

  const title = computed(() => {
    return formatForLocale(messages.title, locale.value)
  })

  const description = computed(() => {
    return formatForLocale(messages.description, locale.value)
  })

  const checklistItems = computed(() => {
    const currentLocale = locale.value
    return checklistBlueprint.map((item) => ({
      id: item.id,
      title: formatForLocale(item.title, currentLocale),
      badge: formatForLocale(item.badge, currentLocale),
      description: formatForLocale(item.description, currentLocale),
    }))
  })

  const coreRecipes = computed(() => {
    const currentLocale = locale.value
    return coreRecipeBlueprint.map((item) => ({
      id: item.id,
      label: item.label,
      title: formatForLocale(item.title, currentLocale),
      manifest: formatForLocale(item.manifest, currentLocale),
      topics: item.topics,
      actions: item.actions.map((action) =>
        formatForLocale(action, currentLocale),
      ),
      note: formatForLocale(item.note, currentLocale),
    }))
  })

  const pitfallItems = computed(() => {
    const currentLocale = locale.value
    return pitfallBlueprint.map((item) => ({
      id: item.id,
      title: formatForLocale(item.title, currentLocale),
      description: formatForLocale(item.description, currentLocale),
    }))
  })

  return {
    locale,
    formatMessage,
    messages,
    title,
    description,
    checklistItems,
    coreRecipes,
    pitfallItems,
  }
}
