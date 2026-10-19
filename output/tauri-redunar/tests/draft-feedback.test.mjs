import test from 'node:test';
import assert from 'node:assert/strict';
import {profileDraftScopes,globalDraftFeedback} from '../ui/draft-feedback.mjs';

const saved={layout:'Grid',palette:'Redunar',branding:true,position:'Top left',scale:100,
 opacity:90,preset:'Compact',metrics:['FPS','CPU load'],overlay:true,captureMetrics:true,
 replay:true,fps:60,quality:'Balanced',format:'MKV'};

test('Reordering selected metrics creates no draft and never mutates the saved profile',()=>{
 const before=structuredClone(saved),current={...saved,metrics:[...saved.metrics].reverse()};
 assert.deepEqual(profileDraftScopes(current,saved),[]);
 assert.deepEqual(saved,before);
 assert.doesNotMatch(globalDraftFeedback(current,saved,false,'overlay'),/unsaved|pending/i);
});

test('Combined drafts name every affected scope once without unrelated scope noise',()=>{
 const current={...saved,scale:110,opacity:75,preset:'Custom',metrics:['FPS'],fps:240};
 const scopes=profileDraftScopes(current,saved);
 for(const label of ['Overlay appearance','Displayed metrics','Replay settings'])
  assert.equal(scopes.filter(scope=>scope===label).length,1);
 assert.equal(scopes.length,3);
 assert.doesNotMatch(globalDraftFeedback(current,saved,false,'overlay'),/Keyboard shortcuts|Overlay visibility|Session metrics/);
});

test('Shortcut feedback distinguishes the active save owner from pending profile changes',()=>{
 const current={...saved,scale:110};
 const shortcut=globalDraftFeedback(current,saved,true,'shortcuts');
 assert.match(shortcut,/Unsaved changes.*Keyboard shortcuts/);
 assert.match(shortcut,/Overlay appearance.*pending separately/);
 const profile=globalDraftFeedback(current,saved,true,'overlay');
 assert.match(profile,/Unsaved changes.*Overlay appearance/);
 assert.match(profile,/Keyboard shortcuts.*pending separately/);
});

test('Saving one owner leaves the other pending and does not claim everything saved',()=>{
 const current={...saved,scale:110};
 const afterShortcutSave=globalDraftFeedback(current,saved,false,'shortcuts');
 assert.match(afterShortcutSave,/Overlay appearance.*pending separately/);
 assert.doesNotMatch(afterShortcutSave,/All changes saved|Keyboard shortcuts/);
 const afterProfileSave=globalDraftFeedback(saved,saved,true,'overlay');
 assert.match(afterProfileSave,/Keyboard shortcuts.*pending separately/);
 assert.doesNotMatch(afterProfileSave,/All changes saved|Overlay appearance/);
});

test('Visibility and session recording drafts remain distinct from appearance',()=>{
 const scopes=profileDraftScopes({...saved,overlay:false,captureMetrics:false},saved);
 assert.deepEqual(new Set(scopes),new Set(['Overlay visibility','Session metrics']));
});
