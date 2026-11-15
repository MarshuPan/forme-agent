import type { WorkbenchClient } from './api/client.ts';
import { acceptedUserInput } from './conversation-view.ts';
import { emptyEvents, parseEvents, reduceEvents } from './events.ts';
import type { EventState } from './events.ts';
import { isFinished } from './types.ts';
import type { RunSummary, WorkbenchSessionRecord } from './types.ts';

export interface ConversationTurn {
  summary: RunSummary; events: EventState; input: string | null; answer: string | null; acceptedAt: number;
}

/** A conversation owns many runs. Run IDs and per-run sequence numbers are not dates. */
export function conversationList(sessions: WorkbenchSessionRecord[], runs: RunSummary[]) {
  return sessions.filter((session) => !session.archived).map((session) => {
    const related = runs.filter((run) => run.session === session.session && run.source === 'UserTurn');
    const active = related.filter((run) => !isFinished(run.status));
    return { ...session, turns: related.length,
      activity: active.some((run) => run.status === 'Waiting') ? '等待处理' : active.length ? '处理中' : null };
  });
}

/** Timestamps order the displayed turns only; each run's facts still use stream_seq. */
export function chronologicalTurns(turns: ConversationTurn[]): ConversationTurn[] {
  return [...turns].sort((left, right) => left.acceptedAt - right.acceptedAt || left.summary.run.localeCompare(right.summary.run));
}

type Reader = Pick<WorkbenchClient, 'run' | 'events' | 'answer'>;
export async function readConversationTurn(
  api: Reader, session: string, run: string, previous?: ConversationTurn, signal?: AbortSignal,
): Promise<ConversationTurn> {
  const summary = await api.run(run, signal);
  if (summary.run !== run || summary.session !== session) throw new Error('消息不属于当前对话，已停止读取。');
  let events = previous?.summary.session === session && previous.summary.run === run ? previous.events : emptyEvents(run);
  events = reduceEvents(events, parseEvents(await api.events(run, events.watermark, signal)));
  if (events.gap) events = reduceEvents(emptyEvents(run), parseEvents(await api.events(run, 0, signal)));
  const input = acceptedUserInput(events, session);
  const accepted = events.events.find((event) => event.kind === 'RunAccepted');
  if (!accepted || !Number.isSafeInteger(accepted.ts_unix_ms)) throw new Error('消息时间尚未确认，请重新读取历史。');
  const output = await api.answer(run, signal);
  if (output.run !== run) throw new Error('回复不属于原消息，已停止读取。');
  return { summary, events, input, answer: output.answer, acceptedAt: accepted.ts_unix_ms! };
}
