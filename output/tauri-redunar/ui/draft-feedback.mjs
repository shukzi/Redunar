// Describe the existing save owner's drafts without changing
// which values Save/Discard commits or how drafts survive navigation.
export function profileDraftScopes(current,saved) {
 const changed=key=>key==='metrics'
  ? JSON.stringify([...(current[key]||[])].sort())!==JSON.stringify([...(saved[key]||[])].sort())
  : current[key]!==saved[key];
 return [
  ['Overlay appearance',['layout','palette','branding','position','scale','opacity']],
  ['Displayed metrics',['preset','metrics']],
  ['Overlay visibility',['overlay']],
  ['Session metrics',['captureMetrics']],
  ['Replay settings',['replay','fps','quality','format']],
 ].filter(([,keys])=>keys.some(changed)).map(([label])=>label);
}
export function globalDraftFeedback(current,saved,shortcutDirty,tab) {
 const profileScopes=profileDraftScopes(current,saved);
 const shortcutScopes=shortcutDirty?['Keyboard shortcuts']:[];
 const currentScopes=tab==='shortcuts'?shortcutScopes:profileScopes;
 const otherScopes=tab==='shortcuts'?profileScopes:shortcutScopes;
 const main=currentScopes.length?`Unsaved changes · ${currentScopes.join(' + ')}`:'';
 const other=otherScopes.length?`${otherScopes.join(' + ')} pending separately`:'';
 return [main,other].filter(Boolean).join(' · ')||'All changes saved';
}
