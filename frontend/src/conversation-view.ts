import type { EventState } from './events.ts';
import { isRecord } from './types.ts';
import type { RunSummary } from './types.ts';

/** The accepted event, not a draft or a second chat store, owns user input. */
export function acceptedUserInput(state: EventState, session: string): string | null {
  if (state.gap) throw new Error('对话事件尚不连续，无法确认原始消息。');
  const accepted = state.events.filter((event) => event.kind === 'RunAccepted');
  if (!accepted.length) return null;
  const event = accepted[0];
  if (accepted.length !== 1 || event.run_id !== state.run || !isRecord(event.payload) ||
      !isRecord(event.payload.RunAccepted)) throw new Error('原始消息的运行绑定不一致。');
  const body = event.payload.RunAccepted;
  if (body.session_ref !== session) throw new Error('原始消息的会话绑定不一致。');
  // Scheduler, subagent and control inputs are not words spoken by the owner.
  if (body.source !== 'UserTurn') return null;
  if (typeof body.input_ref !== 'string') throw new Error('原始消息格式不兼容。');
  return body.input_ref;
}

/** A late list/poll response may never regress a newer authoritative sequence. */
export function mergeObservedRun(runs: RunSummary[], observed: RunSummary): RunSummary[] {
  const index = runs.findIndex((run) => run.run === observed.run);
  if (index < 0) return [...runs, observed];
  if (runs[index].last_stream_seq >= observed.last_stream_seq) return runs;
  return runs.map((run, position) => position === index ? observed : run);
}
