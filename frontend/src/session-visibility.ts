/** A restored workbench stays concealed until a current visibility check finishes. */
export interface SessionVisibilityEnvironment {
  verify(): Promise<unknown>;
  hidden(): boolean;
  conceal(): void;
  reveal(): void;
  invalidated(): void;
}

export function createSessionVisibilityGuard(environment: SessionVisibilityEnvironment) {
  let closed = false;
  let epoch = 0;
  let concealed = false;
  let pending: { epoch: number; completion: Promise<void> } | null = null;

  function check(): Promise<void> {
    if (closed || environment.hidden()) return Promise.resolve();
    if (pending) {
      // A request issued before pagehide cannot authenticate the restored view.
      return pending.epoch === epoch ? pending.completion : pending.completion.then(check);
    }
    const checkingEpoch = epoch;
    const current = () => !closed && checkingEpoch === epoch && !environment.hidden();
    const completion = Promise.resolve().then(async () => {
      // Cleanup or another hide may happen before this microtask starts.
      if (!current()) return;
      try {
        await environment.verify();
        if (!current() || !concealed) return;
        concealed = false;
        environment.reveal();
      } catch {
        // Keep the old workbench hidden until its owner clears and unmounts it.
        if (current()) environment.invalidated();
      }
    }).finally(() => { pending = null; });
    pending = { epoch: checkingEpoch, completion };
    return completion;
  }

  return {
    check,
    hide() {
      if (closed) return;
      epoch++;
      concealed = true;
      environment.conceal();
    },
    show(): Promise<void> {
      if (closed) return Promise.resolve();
      if (!concealed) epoch++;
      concealed = true;
      environment.conceal();
      return check();
    },
    close() {
      if (closed) return;
      closed = true;
      if (concealed) environment.reveal();
      concealed = false;
    },
  };
}
