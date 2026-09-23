// Shape contract shared with the exporter (single source of truth; the
// type-only import is erased at bundle time).
import type ApiV2 from '../src/types/api-v2'
import { handleRequest } from './index'

export type SearchHit = ApiV2.SearchHit

/**
 * Tencent Cloud EdgeOne edge function entry (Service Worker style).
 *
 * Bundle for upload with:  bun run build:eo
 * Deploy (one time):       paste api-worker/dist/eo-function.js into the
 *                          EdgeOne console (or upload via Cloud API) and add
 *                          the trigger rule (conditions combined with AND):
 *                            URL Path starts with  /api/v2/
 *                            URL Path does not contain  .json
 *
 * Everything routing/architecture related is documented in ./index.ts.
 */
interface EdgeOneFetchEvent {
  request: Request
  respondWith(response: Promise<Response> | Response): void
}

addEventListener('fetch', (event) => {
  const e = event as EdgeOneFetchEvent
  e.respondWith(handleRequest(e.request))
})
