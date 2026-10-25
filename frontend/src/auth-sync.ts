const AUTH_CHANNEL = 'forme-auth-state-v1';
const INVALIDATED = Object.freeze({ schema_version: 1, type: 'session-invalidated' });

export interface AuthInvalidationTransport {
  send(message: typeof INVALIDATED): void;
  subscribe(listener: (data: unknown) => void): () => void;
  close(): void;
}

export interface AuthInvalidationSync {
  notifyInvalidated(): void;
  close(): void;
}

function isInvalidationMessage(value: unknown): boolean {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
  const message = value as Record<string, unknown>;
  return message.schema_version === 1
    && message.type === 'session-invalidated'
    && Object.keys(message).length === 2;
}

function createBrowserTransport(): AuthInvalidationTransport | null {
  if (typeof BroadcastChannel !== 'function') return null;
  const channel = new BroadcastChannel(AUTH_CHANNEL);
  return {
    send: (message) => channel.postMessage(message),
    subscribe(listener) {
      const handle = (event: MessageEvent) => listener(event.data);
      channel.addEventListener('message', handle);
      return () => channel.removeEventListener('message', handle);
    },
    close: () => channel.close(),
  };
}

const noop: AuthInvalidationSync = { notifyInvalidated() {}, close() {} };

export function createAuthInvalidationSync(
  onInvalidated: () => void,
  createTransport: () => AuthInvalidationTransport | null = createBrowserTransport,
): AuthInvalidationSync {
  let transport: AuthInvalidationTransport | null;
  try {
    transport = createTransport();
  } catch {
    return noop;
  }
  if (!transport) return noop;

  let unsubscribe: (() => void) | null = null;
  try {
    unsubscribe = transport.subscribe((data) => {
      if (isInvalidationMessage(data)) onInvalidated();
    });
  } catch {
    transport.close();
    return noop;
  }

  let closed = false;
  return {
    notifyInvalidated() {
      if (closed) return;
      try {
        transport?.send(INVALIDATED);
      } catch {
        // Local sign-out must not depend on cross-tab transport availability.
      }
    },
    close() {
      if (closed) return;
      closed = true;
      unsubscribe?.();
      transport?.close();
      unsubscribe = null;
      transport = null;
    },
  };
}
