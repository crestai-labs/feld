// A fixed number of reads in flight at once: a read past the limit waits for one to end,
// in the order they asked. A read whose signal aborts while it waits leaves the line.
export function readSlots(limit: number) {
  let busy = 0;
  const waiting: Array<() => void> = [];

  function acquire(signal?: AbortSignal) {
    if (signal?.aborted) return Promise.reject(signal.reason);
    if (busy < limit) {
      busy += 1;
      return Promise.resolve();
    }
    return new Promise<void>((resolve, reject) => {
      const start = () => {
        signal?.removeEventListener("abort", abort);
        resolve();
      };
      const abort = () => {
        const index = waiting.indexOf(start);
        if (index >= 0) waiting.splice(index, 1);
        reject(signal!.reason);
      };
      waiting.push(start);
      signal?.addEventListener("abort", abort, { once: true });
    });
  }

  // An ending read hands its slot straight to the next in line.
  function release() {
    const next = waiting.shift();
    if (next) next();
    else busy -= 1;
  }

  return async function run<T>(read: () => Promise<T>, signal?: AbortSignal): Promise<T> {
    await acquire(signal);
    try {
      return await read();
    } finally {
      release();
    }
  };
}
