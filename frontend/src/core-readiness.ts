export const CORE_READINESS_RETRY_DELAYS_MS = [500, 1000, 2000, 3000, 5000] as const;

export class CoreReadinessCancelled extends Error {
  constructor() {
    super('core readiness probe was cancelled');
    this.name = 'AbortError';
  }
}

export type CoreReadinessOptions = {
  signal?: AbortSignal;
  retryDelaysMs?: readonly number[];
  shouldRetry?: (error: unknown) => boolean;
  sleep?: (delayMs: number, signal?: AbortSignal) => Promise<void>;
};

function statusOf(error: unknown): number | null {
  if (!error || typeof error !== 'object' || !('status' in error)) return null;
  const status = (error as { status?: unknown }).status;
  return typeof status === 'number' ? status : null;
}

/** Only transient service availability failures are safe to probe again. */
export function isRetryableCoreError(error: unknown): boolean {
  const status = statusOf(error);
  return status === 0 || status === 408 || status === 425 || status === 500 || status === 502 || status === 503 || status === 504;
}

function sleep(delayMs: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new CoreReadinessCancelled());
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener('abort', abort);
      resolve();
    }, delayMs);
    function abort() {
      clearTimeout(timer);
      reject(new CoreReadinessCancelled());
    }
    signal?.addEventListener('abort', abort, { once: true });
  });
}

export async function waitForCoreReady<T>(probe: () => Promise<T>, options: CoreReadinessOptions = {}): Promise<T> {
  const delays = options.retryDelaysMs ?? CORE_READINESS_RETRY_DELAYS_MS;
  const shouldRetry = options.shouldRetry ?? isRetryableCoreError;
  const wait = options.sleep ?? sleep;
  let attempt = 0;

  while (true) {
    if (options.signal?.aborted) throw new CoreReadinessCancelled();
    try {
      return await probe();
    } catch (error) {
      if (options.signal?.aborted) throw new CoreReadinessCancelled();
      const delay = delays[attempt];
      if (delay === undefined || !shouldRetry(error)) throw error;
      attempt += 1;
      await wait(delay, options.signal);
    }
  }
}
