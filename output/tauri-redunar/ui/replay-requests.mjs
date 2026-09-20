// Playback preparation is expensive and changes the native stream token.
// Keep one request in flight and only the newest pending selection, so an
// obsolete completion cannot load a video or invalidate a newer stream.
export function latestReplayRequest(run, cancel = () => {}) {
  let revision = 0, pending = null, running = false, cancelRequested = false;
  async function drain() {
    if (running) return;
    running = true;
    try {
      while (pending) {
        const request = pending;
        pending = null;
        cancelRequested = false;
        try {
          const result = await run(request.value);
          if (request.revision === revision) request.accept(result);
        } catch (error) {
          if (request.revision === revision) request.reject(error);
        }
      }
    } finally {
      running = false;
    }
  }
  return {
    request(value, accept, reject) {
      pending = {value, accept, reject, revision: ++revision};
      if (running && !cancelRequested) { cancelRequested = true; cancel(); }
      void drain();
    },
    invalidate() {
      revision++;
      pending = null;
      if (running && !cancelRequested) { cancelRequested = true; cancel(); }
    },
  };
}
