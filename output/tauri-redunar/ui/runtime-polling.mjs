// Keep update/download latency independent of native session presentation.
export async function startRuntime({initialize, refresh, render, checkUpdates, updatePresentation, startPolling, onError}) {
  try { await initialize(); await refresh(); } catch (error) { onError(error); }
  render();
  startPolling();
  // The update result owns only its card/badge. Rebuilding the workspace here
  // cancels a clip preparation or discards focus after unrelated navigation.
  void Promise.resolve().then(checkUpdates).then(updatePresentation).catch(onError);
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

// Hidden windows need no periodic UI wakeups. Native supervision is independent.
export function pollVisibleRuntime(page,refresh,delay,onError) {
 let timer=null, reading=false;
 function schedule(wait=delay()) {
  clearTimeout(timer);timer=null;
  if(!page.hidden&&!reading)timer=setTimeout(read,wait);
 }
 async function read() {
  timer=null;
  if(page.hidden)return;
  reading=true;
  try { await refresh(); } catch(error) { onError(error); }
  finally { reading=false;schedule(); }
 }
 page.addEventListener('visibilitychange',()=>schedule(0));
 // The startup read just completed. Wait the normal interval.
 schedule();
}
