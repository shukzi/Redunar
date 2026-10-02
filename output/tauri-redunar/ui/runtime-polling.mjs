// Keep update/download latency independent of native session presentation.
export async function startRuntime({initialize, refresh, render, checkUpdates, startPolling, onError}) {
  try { await initialize(); await refresh(); } catch (error) { onError(error); }
  render();
  startPolling();
  void Promise.resolve().then(checkUpdates).then(render).catch(onError);
}

// Actions and the periodic poll share one reader. Retain at most one follow-up
// so a save arriving during a read gets fresh state without out-of-order writes.
export function latestRuntimeRefresh(read) {
  let inFlight = null, requested = false;
  return () => {
    requested = true;
    if (!inFlight) inFlight = Promise.resolve().then(async () => {
      do { requested = false; await read(); } while (requested);
    }).finally(() => { inFlight = null; });
    return inFlight;
  };
}
