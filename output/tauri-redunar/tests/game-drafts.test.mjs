import test from 'node:test';
import assert from 'node:assert/strict';
import { mergeGameDrafts } from '../ui/game-drafts.mjs';
const saved = [{id:'a',revision:'a1',overrides:{}},{id:'b',revision:'b1',overrides:{overlay:false}}];
const base = () => new Map(saved.map(g=>[g.id,{...g.overrides}]));
test('saving B retains the unsaved A profile',()=>{
 const current=structuredClone(saved);current[0].overrides.overlay=false;current[1].overrides.overlay=true;
 const response=structuredClone(saved);response[1].overrides.overlay=true;response[1].revision='b2';
 const merged=mergeGameDrafts(response,current,base(),'b');
 assert.equal(merged[0].overrides.overlay,false);assert.equal(merged[1].revision,'b2');
 assert.deepEqual(response[0].overrides,{});
});
test('launch edits and imports retain both a pending override and reset to inheritance',()=>{
 const current=structuredClone(saved);current[0].overrides.captureMetrics=false;delete current[1].overrides.overlay;
 const response=structuredClone(saved);response[0].executable='/new/path';response.push({id:'c',revision:'c1',overrides:{}});
 const merged=mergeGameDrafts(response,current,base());
 assert.equal(merged[0].overrides.captureMetrics,false);assert.equal(merged[0].executable,'/new/path');
 assert.deepEqual(merged[1].overrides,{});assert.equal(merged.length,3);
});
test('concurrent saved revisions do not silently overwrite a dirty edit',()=>{
 const current=structuredClone(saved);current[0].overrides.overlay=false;
 const response=structuredClone(saved);response[0].revision='a2';response[0].overrides.captureMetrics=false;
 const merged=mergeGameDrafts(response,current,base());
 assert.equal(merged[0].revision,'a1');assert.equal(merged[0].savedRevision,'a2');
 assert.equal(merged[0].overrides.overlay,false);assert.equal(merged[0].overrides.captureMetrics,false);
});
