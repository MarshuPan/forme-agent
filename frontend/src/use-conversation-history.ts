import { useEffect, useRef, useState } from 'react';
import type { WorkbenchClient } from './api/client.ts';
import { ApiError } from './api/client.ts';
import { chronologicalTurns, readConversationTurn } from './conversation-history.ts';
import type { ConversationTurn } from './conversation-history.ts';
import { readDelay } from './events.ts';
import { isFinished } from './types.ts';
import type { RunSummary } from './types.ts';

/** Read-only, scoped in-memory history. At most two runs are read concurrently. */
export function useConversationHistory(api: WorkbenchClient, session: string | null, runs: RunSummary[],
  selected: string | null, active: boolean, revision: number, observed: (run: RunSummary) => void) {
  const cache = useRef({ session, turns: new Map<string, ConversationTurn>() });
  const previousRead = useRef({ session, revision, active });
  const [view, setView] = useState<{ session: string | null; turns: ConversationTurn[]; loading: boolean; error: string }>(
    { session, turns: [], loading: false, error: '' });
  const ids = JSON.stringify([...new Set([...runs.filter((run) => run.session === session && run.source === 'UserTurn').map((run) => run.run), ...(selected ? [selected] : [])])].sort());

  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined, attempt = 0;
    const refreshExisting = previousRead.current.session !== session || previousRead.current.revision !== revision || !previousRead.current.active;
    previousRead.current = { session, revision, active };
    if (cache.current.session !== session) cache.current = { session, turns: new Map() };
    const stored = cache.current.turns;
    const runIds: string[] = JSON.parse(ids);
    const failed = new Set<string>();
    const publish = (loading: boolean, error = '') => {
      if (!controller.signal.aborted) setView({ session, turns: chronologicalTurns([...stored.values()].filter((turn) => runIds.includes(turn.summary.run))), loading, error });
    };
    publish(Boolean(session && active && runIds.length));
    if (!session || !active) return () => controller.abort();
    async function poll(first = false) {
      let failure: unknown;
      const pending = runIds.filter((id) => first || failed.has(id) || !stored.has(id) || !isFinished(stored.get(id)!.summary.status));
      for (let offset = 0; offset < pending.length && !controller.signal.aborted; offset += 2) {
        const batch = pending.slice(offset, offset + 2);
        const results = await Promise.allSettled(batch.map((id) => readConversationTurn(api, session!, id, first ? undefined : stored.get(id), controller.signal)));
        if (controller.signal.aborted) return;
        for (const [index, result] of results.entries()) {
          if (result.status === 'fulfilled') {
            failed.delete(batch[index]);
            stored.set(result.value.summary.run, result.value);
            observed(result.value.summary);
          } else { failed.add(batch[index]); failure = result.reason; }
        }
        // Respect throttling/authentication without continuing through the list.
        if (failure instanceof ApiError && [401, 429].includes(failure.status)) break;
      }
      if (controller.signal.aborted) return;
      publish(false, failure ? failure instanceof Error ? failure.message : '部分对话记录暂不可读。' : '');
      if (failure instanceof ApiError && failure.status === 401) return;
      if (failure || runIds.some((id) => !stored.has(id) || !isFinished(stored.get(id)!.summary.status))) {
        const delay = failure ? readDelay(failure, attempt++) : 2500;
        if (!failure) attempt = 0;
        timer = setTimeout(() => void poll(), delay);
      }
    }
    void poll(refreshExisting);
    return () => { controller.abort(); if (timer) clearTimeout(timer); };
  }, [api, session, ids, active, revision, observed]);
  return view.session === session ? view : { session, turns: [], loading: true, error: '' };
}
