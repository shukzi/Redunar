import {FPS_ONLY, selectedOverlayLayout} from './overlay-layout.mjs';
import {draftFieldChanged,profileDraftGroups} from './native-data.mjs';

// Describe the existing save owner's drafts without changing
// which values Save/Discard commits or how drafts survive navigation.
export function profileDraftScopes(current,saved) {
 const changed=key=>draftFieldChanged(current,saved,key);
 const layoutChanged=selectedOverlayLayout(current)!==selectedOverlayLayout(saved);
 const panelPresetChanged=current.preset!==FPS_ONLY&&saved.preset!==FPS_ONLY&&changed('preset');
 return [
  ['Overlay appearance',layoutChanged||profileDraftGroups.appearance.some(changed)],
  ['Displayed metrics',panelPresetChanged||changed('metrics')],
  ['Overlay visibility',profileDraftGroups.visibility.some(changed)],
  ['Session metrics',profileDraftGroups.session.some(changed)],
  ['Replay settings',profileDraftGroups.replay.some(changed)],
 ].filter(([,dirty])=>dirty).map(([label])=>label);
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
