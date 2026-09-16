// Playback preparation is expensive and changes the native stream token.
// Keep one request in flight and only the newest pending selection, so an
// obsolete completion cannot load a video or invalidate a newer stream.
export function latestReplayRequest(run) {
  let revision = 0, pending = null, running = false;
  async function drain() {
    if (running) return;
    running = true;
    try {
      while (pending) {
        const request = pending;
        pending = null;
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
      void drain();
    },
    invalidate() {
      revision++;
      pending = null;
    },
  };
}
