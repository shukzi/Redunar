const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const requests = new WeakMap();

export function steamSetupPanel() {
  return `<section class="steam-setup-card" aria-labelledby="steam-setup-title"><h3 id="steam-setup-title">Play from Steam</h3><div data-steam-setup-content><p>Checking Steam capture setup…</p></div></section>`;
}

const retry = '<button class="button steam-setup-retry" type="button" data-action="check-steam-setup">Check again</button>';

function setupContent(setup) {
  if (!setup.available) return `<p class="steam-setup-status">Steam capture setup unavailable</p><p>${escape(setup.status)}</p>${retry}`;
  const configured = setup.configured === true;
  const needsAttention = !configured && !['not-configured', 'steam-running'].includes(setup.configuration_state);
  const options = setup.launch_options || '';
  return `${configured ? '<p class="steam-setup-status">Launch option configured</p>' : ''}<p>${configured
    ? 'Press Play in Steam. Redunar starts automatically.'
    : 'Paste this into Steam’s Launch Options, then press Play.'}</p>
    ${options ? `<div class="steam-options-field"><textarea id="steam-launch-options" aria-label="Steam Launch Options" rows="2" readonly spellcheck="false">${escape(options)}</textarea><button class="button primary" type="button" data-copy-steam-options="${escape(options)}">Copy</button></div>` : ''}
    ${!configured ? '<p class="steam-setup-caption">Set up once. Redunar starts automatically with the game.</p>' : ''}
    ${needsAttention ? `<div class="steam-setup-notice"><p class="steam-setup-status">Setup unconfirmed</p><p>${escape(setup.status)}</p>${retry}</div>` : ''}`;
}

// Keep late checks scoped to the panel that requested them. Never replace the
// game/profile workspace or let an earlier retry overwrite newer evidence.
export async function refreshSteamSetup(panel, gameId, call) {
  if (!panel) return;
  const request = {markup:requests.get(panel)?.markup};
  requests.set(panel, request);
  const content = panel.querySelector('[data-steam-setup-content]');
  const button = panel.querySelector('[data-action="check-steam-setup"]');
  const update = markup => {
    if (!panel.isConnected || requests.get(panel) !== request || request.markup === markup) return;
    content.innerHTML = markup;
    request.markup = markup;
  };
  if (button) button.disabled = true;
  try {
    const setup = await call('steam_setup_status', {gameId});
    update(setupContent(setup));
  } catch (error) {
    const note = error instanceof Error ? error.message : String(error);
    update(`<p class="steam-setup-status">Steam setup could not be checked</p><p>${escape(note)}</p>${retry}`);
  } finally {
    if (panel.isConnected && requests.get(panel) === request) {
      const retryButton = panel.querySelector('[data-action="check-steam-setup"]');
      if (retryButton) retryButton.disabled = false;
    }
  }
}
