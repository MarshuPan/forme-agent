import { isRecord, versioned } from './types.ts';
import type { BrainEvent } from './types.ts';

export interface EventState { run: string; watermark: number; events: BrainEvent[]; gap: boolean }
export const emptyEvents = (run: string): EventState => ({ run, watermark: 0, events: [], gap: false });

/** stream_seq is authoritative within one run. Arrival time is irrelevant. */
export function reduceEvents(state: EventState, events: BrainEvent[]): EventState {
  let watermark = state.watermark;
  const accepted: BrainEvent[] = [];
  for (const event of events) {
    versioned(event);
    if (event.run_id !== state.run || !Number.isSafeInteger(event.stream_seq) || event.stream_seq < 1) throw new Error('事件不属于当前运行或序号无效。');
    if (event.stream_seq <= watermark) {
      const existing = state.events.find((item) => item.stream_seq === event.stream_seq) ?? accepted.find((item) => item.stream_seq === event.stream_seq);
      if (existing && existing.event_id !== event.event_id) throw new Error('同一事件序号的身份不一致，需要重新同步。');
      continue;
    }
    if (event.stream_seq !== watermark + 1) return { ...state, gap: true };
    if (state.events.some((item) => item.event_id === event.event_id) || accepted.some((item) => item.event_id === event.event_id)) throw new Error('事件身份重复但序号不同。');
    accepted.push(event); watermark = event.stream_seq;
  }
  return { ...state, watermark, events: [...state.events, ...accepted], gap: false };
}

export function parseEvents(text: string): BrainEvent[] {
  return text.replaceAll('\r\n', '\n').split('\n\n').filter((block) => block.trim()).flatMap((block) => {
    const data = block.split('\n').filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trimStart()).join('\n');
    if (!data) return [];
    const value: unknown = JSON.parse(data); versioned(value);
    if (typeof value.event_id !== 'string' || typeof value.kind !== 'string' || !isRecord(value.payload)) throw new Error('事件格式不兼容。');
    return [value as unknown as BrainEvent];
  });
}

export function readDelay(error: unknown, attempt: number, random = Math.random): number {
  const retry = isRecord(error) && typeof error.retryAfterMs === 'number' ? error.retryAfterMs : 0;
  return Math.max(retry, Math.min(30_000, 1000 * 2 ** Math.min(attempt, 5)) * (0.8 + random() * 0.4));
}
