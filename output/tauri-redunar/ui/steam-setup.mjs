const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const requests = new WeakMap();

export function steamSetupPanel() {
  return `<section class="steam-setup-card" aria-labelledby="steam-setup-title"><div class="steam-setup-heading"><h3 id="steam-setup-title">Play from Steam</h3><button class="button" data-action="check-steam-setup">Check again</button></div><div data-steam-setup-content><p>Checking Steam capture setup…</p></div></section>`;
}

function setupContent(setup) {
  if (!setup.available) return `<p class="steam-setup-status">Steam capture setup unavailable</p><p>${escape(setup.status)}</p>`;
  const configured = setup.configured === true;
  const status = configured ? 'Launch options configured' : setup.configuration_state === 'not-configured' ? 'Setup required' : 'Setup unconfirmed';
  const options = setup.launch_options || '';
  return `<p class="steam-setup-status">${status}</p>${configured
    ? '<p>Click Play in Steam. Redunar starts in the background automatically and uses this game’s saved settings.</p>'
    : '<p>Set up this game once to use Redunar’s overlay and Instant Replay when you click Play in Steam.</p><ol class="steam-setup-steps"><li>In Steam, open this game’s <strong>Properties → General → Launch Options</strong>.</li><li>Copy the value below into that field, then click <strong>Play</strong>. Redunar starts in the background automatically.</li></ol>'}
    ${!configured && setup.configuration_state !== 'not-configured' ? `<p class="small-note">${escape(setup.status)} Check again after Steam saves the field. The updated option can also prepare capture before it is saved.</p>` : ''}
    ${options ? `<label class="form-label" for="steam-launch-options">Required Steam Launch Options</label><div class="steam-options-field"><textarea id="steam-launch-options" rows="2" readonly spellcheck="false">${escape(options)}</textarea><button class="button" type="button" data-copy-steam-options="${escape(options)}">Copy launch option</button></div>` : ''}`;
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
  button.disabled = true;
  try {
    const setup = await call('steam_setup_status', {gameId});
    update(setupContent(setup));
  } catch (error) {
    const note = error instanceof Error ? error.message : String(error);
    update(`<p class="steam-setup-status">Steam setup could not be checked</p><p>${escape(note)}</p>`);
  } finally {
    if (panel.isConnected && requests.get(panel) === request) button.disabled = false;
  }
}
