import { useEffect, useRef, useState } from 'react';
import { ExactOperation } from '../exact-operation.ts';
import type { ExactRequest } from '../exact-operation.ts';

export type PendingChange = (name: string, pending: boolean) => void;
export function useProjectCommand<T>(name: string, changed: PendingChange) {
  const operation = useRef(new ExactOperation<T>()).current;
  const [, render] = useState(0);
  const [error, setError] = useState('');
  useEffect(() => { changed(name, operation.pending || operation.busy); }, [changed, name, operation, operation.pending, operation.busy]);
  async function run(create: () => ExactRequest<T> | Promise<ExactRequest<T>>, dispatch: (request: ExactRequest<T>) => Promise<unknown>): Promise<boolean> {
    setError('');
    const promise = operation.run(create, dispatch);
    render((value) => value + 1);
    try { await promise; return true; }
    catch (problem) { setError(problem instanceof Error ? problem.message : '操作尚未确认，请核实原命令。'); return false; }
    finally { render((value) => value + 1); }
  }
  function reloadConflict() { operation.releaseConflict(); setError(''); render((value) => value + 1); }
  return { operation, error, run, reloadConflict };
}
