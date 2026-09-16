// Refresh installation evidence without replacing catalog records or unsaved drafts.
export function gameInstallation(call, changed) {
  let statuses = {}, failed = false, pending = null, refreshAgain = false;
  return {
    status(id) { return statuses[id] || (failed ? 'unknown' : 'checking'); },
    refresh() {
      if (pending) { refreshAgain = true; return pending; }
      pending = (async () => {
        do {
          refreshAgain = false;
          try { statuses = await call('game_installation_statuses'); failed = false; }
          catch { statuses = {}; failed = true; }
          changed();
        } while (refreshAgain);
      })().finally(() => { pending = null; });
      return pending;
    },
  };
}
