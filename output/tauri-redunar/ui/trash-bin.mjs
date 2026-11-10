const escape = value => String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const size = bytes => `${(bytes/1000000).toFixed(1)} MB`;
export function trashBinMarkup(items) {
 const rows=items.map(item=>`<div class="trash-bin-row"><div><strong>${escape(item.file_name)}</strong><small>${size(item.bytes)}</small></div><div class="trash-bin-actions"><button class="button" data-action="restore-trash-clip" data-file-name="${escape(item.file_name)}">Restore</button><button class="button" data-action="delete-trash-clip" data-file-name="${escape(item.file_name)}">Delete permanently</button></div></div>`).join('');
 return `${items.length?`<p>Trashed clips still use disk space. Permanent deletion cannot be undone.</p><div class="trashed-clip-list">${rows}</div>`:'<p>The trash bin is empty.</p>'}<div class="dialog-actions">${items.length?'<button class="button" data-action="clear-trash-bin">Clear bin</button>':''}<button class="button" type="button" data-close>Done</button></div>`;
}
export function trashDeletionMarkup(preview) {
 const total=preview.entries.reduce((sum,item)=>sum+item.bytes,0);
 return `<p>Permanently delete ${preview.entries.length} ${preview.entries.length===1?'clip':'clips'} (${size(total)})? This cannot be undone.</p><div class="trash-delete-list">${preview.entries.map(item=>`<p>${escape(item.file_name)}</p>`).join('')}</div><div class="dialog-actions"><button class="button" data-action="view-clip-trash">Cancel</button><button class="button primary" data-action="confirm-trash-deletion" data-token="${escape(preview.token)}">Delete permanently</button></div>`;
}
export function trashDeletionResultMarkup(result) {
 return `<p>${result.deleted.length} ${result.deleted.length===1?'clip permanently deleted':'clips permanently deleted'}.</p>${result.failures.length?`<p>Some items need attention. Refresh the bin before trying again.</p><div class="trash-delete-list">${result.failures.map(([name,error])=>`<p><strong>${escape(name)}</strong><br>${escape(error)}</p>`).join('')}</div>`:''}<div class="dialog-actions"><button class="button primary" data-action="view-clip-trash">Back to bin</button></div>`;
}
