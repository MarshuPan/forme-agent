import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { ExactOperation } from '../exact-operation.ts';
import type { WorkbenchSessionMutation, WorkbenchSessionRecord } from '../types.ts';

interface Props {
  api: WorkbenchClient;
  session: WorkbenchSessionRecord;
  locked: boolean;
  lock: (locked: boolean) => void;
  refresh: () => Promise<void>;
  confirmed: (session: WorkbenchSessionRecord) => Promise<void>;
}

type Edit = { kind: 'rename' | 'archive'; original: WorkbenchSessionRecord };
type Request = { session: string; updatedAt: number; mutation: WorkbenchSessionMutation };

/** Page forms also work in browsers that do not provide script dialogs. */
export function SessionActions({ api, session, locked, lock, refresh, confirmed }: Props) {
  const [edit, setEdit] = useState<Edit | null>(null);
  const [title, setTitle] = useState('');
  const [error, setError] = useState('');
  const [operation] = useState(() => new ExactOperation<Request>());
  const [, redraw] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const wasOpen = useRef(false);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; lock(false); };
  }, [lock]);
  useEffect(() => {
    if (edit?.kind === 'rename') input.current?.focus();
    else if (edit) heading.current?.focus();
    else if (wasOpen.current) trigger.current?.focus();
    wasOpen.current = edit !== null;
  }, [edit]);

  function open(kind: Edit['kind'], button: HTMLButtonElement) {
    if (locked || edit) return;
    trigger.current = button;
    setEdit({ kind, original: structuredClone(session) });
    setTitle(session.title); setError(''); lock(true);
  }
  function close() {
    if (operation.busy || operation.pending) return;
    setEdit(null); setError(''); lock(false);
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!edit || locked || operation.busy || operation.conflict) return;
    const nextTitle = title.trim();
    if (!operation.pending && edit.kind === 'rename') {
      if (!nextTitle || new TextEncoder().encode(nextTitle).length > 200 || /[\u0000-\u001f\u007f-\u009f]/u.test(nextTitle)) {
        setError('名称不能为空，不能包含控制字符，且最多为 200 字节。'); return;
      }
      if (nextTitle === edit.original.title) { close(); return; }
    }
    setError('');
    let result: WorkbenchSessionRecord | undefined;
    const request = operation.run(() => ({ key: newKey('session-update'), payload: {
      session: edit.original.session, updatedAt: edit.original.updated_at,
      mutation: edit.kind === 'rename' ? { operation: 'rename', title: nextTitle } : { operation: 'archive', archived: true },
    } }), async ({ key, payload }) => {
      result = await api.updateWorkbenchSession(payload.session, payload.updatedAt, payload.mutation, key);
    });
    redraw((value) => value + 1);
    try {
      await request;
      if (!alive.current || !result) return;
      setEdit(null); lock(false);
      // The mutation is confirmed even if the following list refresh fails.
      await confirmed(result);
    } catch (problem) {
      if (alive.current) setError(problem instanceof Error ? problem.message : '会话操作结果尚未确认，请核实原操作。');
    } finally { if (alive.current) redraw((value) => value + 1); }
  }
  async function reloadConflict() {
    if (operation.busy || !operation.conflict) return;
    try {
      await refresh();
      if (!alive.current) return;
      operation.releaseConflict(); setEdit(null); setError(''); lock(false);
    } catch (problem) {
      if (alive.current) setError(problem instanceof Error ? problem.message : '会话状态仍未读取成功。');
    }
  }

  return <div className="session-actions">
    <div className="button-row">
      <button className="text-button" disabled={locked || edit !== null} onClick={(event) => open('rename', event.currentTarget)}>重命名</button>
      <button className="text-button" disabled={locked || edit !== null || session.archived} onClick={(event) => open('archive', event.currentTarget)}>归档</button>
    </div>
    {edit && <form className="surface session-edit" aria-label={edit.kind === 'rename' ? '重命名会话' : '归档会话'} onSubmit={(event) => void submit(event)} onKeyDown={(event) => {
      if (event.key === 'Escape') { event.preventDefault(); close(); }
    }}>
      <h2 ref={heading} tabIndex={-1}>{edit.kind === 'rename' ? '重命名会话' : '归档当前会话'}</h2>
      {edit.kind === 'rename' ? <label>会话名称<input ref={input} value={title} maxLength={200} disabled={operation.pending || operation.busy} onChange={(event) => setTitle(event.target.value)} required /></label>
        : <p>归档“{edit.original.title}”后，该会话将从列表移出。运行记录与附件仍然保留。</p>}
      {operation.pending && <p className="notice" role="status">保留原会话、版本和命令标识，请先核实这次操作。</p>}
      <div className="button-row">
        {operation.conflict ? <button type="button" className="secondary-button" onClick={() => void reloadConflict()}>重新读取会话</button>
          : <button className="primary-button" disabled={operation.busy || locked}>{operation.busy ? '正在核实…' : operation.pending ? '核实原操作' : edit.kind === 'rename' ? '保存名称' : '确认归档'}</button>}
        <button type="button" className="secondary-button" disabled={operation.busy || operation.pending} onClick={close}>取消</button>
      </div>
    </form>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>;
}
