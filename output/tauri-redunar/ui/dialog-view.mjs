import {icon} from './icons.mjs';
// Shared dialog presentation. Native commands remain owned by the caller.
const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const symbols = {standard:'standard',confirm:'confirm',success:'success',warning:'warning'};

export function dialogHeader(title, kind = 'standard', {titleId = 'dialog-title'} = {}) {
 const tone = Object.hasOwn(symbols, kind) ? kind : 'standard';
 return `<div class="dialog-heading"><span class="dialog-emblem" data-kind="${tone}" aria-hidden="true">${icon(symbols[tone])}</span><h2${titleId ? ` id="${escape(titleId)}"` : ''}>${escape(title)}</h2></div>`;
}

export function restartConfirmationContent() {
 return '<p>Your settings have changed but are not saved.</p><p class="dialog-supporting">Choose whether to keep editing, restart without those changes, or save them before restarting.</p><div class="dialog-actions"><button class="button" type="button" data-close>Keep editing</button><button class="button" type="button" data-action="discard-and-restart">Restart without saving</button><button class="button primary" type="button" data-action="save-and-restart">Save changes &amp; restart</button></div>';
}

export function exportCompleteContent({fileName, destination, duration, size}) {
 return `<div class="export-complete"><strong>${escape(fileName)}</strong><p>${escape(destination)}</p><small>${escape(duration)} · ${escape(size)}</small></div><div class="dialog-actions"><button class="button" type="button" data-close>Done</button><button class="button" type="button" data-action="show-export-folder" data-file-name="${escape(fileName)}">Show in folder</button><button class="button primary" type="button" data-action="open-exported-clip" data-file-name="${escape(fileName)}">Open clip</button></div>`;
}
