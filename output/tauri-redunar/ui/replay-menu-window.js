import { invoke } from '@tauri-apps/api/core';
import { renderReplayMenu, updateReplayMenu } from './replay-menu-view.mjs';

// This entry point deliberately has no desktop router, catalog, media player,
// or session-history startup work. Its document contains only the replay menu.
const root = document.querySelector('#replay-menu-root');
root.innerHTML = renderReplayMenu(null);
let runtime = null;
let preferences = null;
let saving = false;
let refreshing = false;
let refreshAgain = false;
let timer;
const feedback = root.querySelector('#replay-menu-feedback');
const title = root.querySelector('h1');
title.tabIndex = -1;
const report = text => {
  feedback.dataset.manual = 'true';
  feedback.textContent = text;
};
const errorText = error => error instanceof Error ? error.message : String(error);

async function refresh() {
  clearTimeout(timer);
  if (refreshing) { refreshAgain = true; return; }
  if (document.hidden) return;
  refreshing = true;
  try {
    const [status, settings] = await Promise.allSettled([
      invoke('replay_runtime_status'), invoke('replay_preferences'),
    ]);
    runtime = status.status === 'fulfilled' ? status.value : null;
    preferences = settings.status === 'fulfilled' ? settings.value : null;
    updateReplayMenu(root, runtime, saving);
  } finally {
    refreshing = false;
    const delay = refreshAgain ? 0 : 1000;
    refreshAgain = false;
    if (!document.hidden) timer = setTimeout(refresh, delay);
  }
}
async function close() {
  try { await invoke('hide_replay_menu'); }
  catch (error) { report(errorText(error)); }
}
root.addEventListener('click', async event => {
  if (event.target.closest('[data-action="hide-replay-menu"]')) { await close(); return; }
  const button = event.target.closest('[data-menu-duration]');
  if (!button || button.disabled || saving) return;
  saving = true;
  updateReplayMenu(root, runtime, saving);
  report('Saving the selected replay…');
  try {
    await invoke('save_replay', { durationSeconds: Number(button.dataset.menuDuration) });
    report('Replay save requested. Return to your game when ready.');
  } catch (error) { report(errorText(error)); }
  finally { saving = false; await refresh(); }
});
document.addEventListener('pointerdown', event => {
  if (preferences?.close_overlay_on_outside_click && !event.target.closest('.replay-menu-panel')) close();
});
document.addEventListener('keydown', event => {
  if (event.key === 'Escape') { event.preventDefault(); close(); }
});
function reopen() {
  if (document.hidden) { clearTimeout(timer); return; }
  if (!saving) delete feedback.dataset.manual;
  title.focus({ preventScroll: true });
  refresh();
}
window.addEventListener('focus', reopen);
document.addEventListener('visibilitychange', reopen);
reopen();
