export type CleanupFn = () => void;

export function createAsyncCleanupScope() {
  let disposed = false;
  const pending: Promise<CleanupFn>[] = [];

  return {
    register(promise: Promise<CleanupFn>) {
      pending.push(promise);

      void promise.then((cleanup) => {
        if (disposed) {
          cleanup();
        }
      });
    },
    dispose() {
      disposed = true;

      for (const promise of pending) {
        void promise.then((cleanup) => {
          cleanup();
        });
      }
    },
  };
}
