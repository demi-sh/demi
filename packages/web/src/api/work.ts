import type { WorkBackend, WorkChange } from '@demicodes/web-ui/agent/work-changes'
import { apiRequest, jsonBody, readResponse } from './client'
import { workAnswerSchema, workChangesSchema } from './generated/web-api'

/** A change as the work's route names it; the route's schema checks it before it leaves. */
function changeBody(change: WorkChange) {
  switch (change.type) {
    case 'create':
      return {
        op: 'create',
        id: change.tab.id,
        kind: change.tab.kind,
        data: change.tab.data,
        ...(change.index === undefined ? {} : { index: change.index }),
      }
    case 'update':
      return { op: 'update', id: change.id, data: change.data }
    case 'remove':
      return { op: 'delete', id: change.id }
    case 'move':
      return { op: 'move', id: change.id, index: change.index }
    case 'windows':
      return { op: 'windows', windows: change.windows }
  }
}

/** The conversation's work's changes over its route; the conversation's entry in the list brings the work. */
export function workBackend(conversationId: string): WorkBackend {
  const work = `/conversations/${encodeURIComponent(conversationId)}/work`
  return {
    async send(changes) {
      // A kind's data is JSON the page made; the schema says so before it leaves.
      const body = workChangesSchema.parse({ changes: changes.map(changeBody) })
      return readResponse(await apiRequest(`${work}/changes`, { method: 'POST', ...jsonBody(body) }), workAnswerSchema)
    },
  }
}
