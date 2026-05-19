const RELOAD_STORAGE_KEY = 'nukkithub:stale-build-reload'
const RELOAD_QUERY_PARAM = 'nukkithub-refresh'
const RELOAD_TTL_MS = 10_000

function isStaleBuildError(error: unknown): boolean {
  const message =
    error instanceof Error
      ? `${error.message}\n${error.stack ?? ''}`
      : String(error ?? '')

  return (
    message.includes('Failed to fetch dynamically imported module') ||
    message.includes('/_nuxt/builds/meta/') ||
    message.includes('Error fetching app manifest')
  )
}

function reloadOnceForStaleBuild(error: unknown): void {
  if (import.meta.server || !isStaleBuildError(error)) return

  const now = Date.now()
  const url = new URL(window.location.href)

  try {
    const lastReload = Number(sessionStorage.getItem(RELOAD_STORAGE_KEY) ?? 0)
    if (now - lastReload < RELOAD_TTL_MS) return

    sessionStorage.setItem(RELOAD_STORAGE_KEY, String(now))
  } catch {
    if (url.searchParams.has(RELOAD_QUERY_PARAM)) return
  }

  url.searchParams.set(RELOAD_QUERY_PARAM, String(now))
  window.location.replace(url)
}

export default defineNuxtPlugin((nuxtApp) => {
  nuxtApp.hook('app:mounted', () => {
    const url = new URL(window.location.href)
    if (!url.searchParams.has(RELOAD_QUERY_PARAM)) return

    url.searchParams.delete(RELOAD_QUERY_PARAM)
    window.history.replaceState(window.history.state, '', url)
  })

  nuxtApp.hook('app:error', (error: any) => {
    console.error('=== APP ERROR ===')
    console.error('Message:', error?.message)
    console.error('Stack:', error?.stack)
    console.error('Full error:', error)
    reloadOnceForStaleBuild(error)
  })

  nuxtApp.hook('vue:error', (error: any, instance: any, info: string) => {
    console.error('=== VUE ERROR ===')
    console.error('Info:', info)
    console.error('Message:', error?.message)
    console.error('Stack:', error?.stack)
    console.error(
      'Component:',
      instance?.$options?.name || instance?.type?.name,
    )
    reloadOnceForStaleBuild(error)
  })

  nuxtApp.hook('app:chunkError', ({ error }) => {
    reloadOnceForStaleBuild(error)
  })
})
