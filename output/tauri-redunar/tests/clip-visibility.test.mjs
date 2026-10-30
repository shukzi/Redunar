import {test} from 'node:test';
import assert from 'node:assert/strict';
import {clipVisibilityTracker} from '../ui/clip-visibility.mjs';

function fixture(){
 let updates=0;const observers=[];
 const tracker=clipVisibilityTracker({changed:()=>updates++,
  makeObserver(callback,options){const observed=[],value={callback,options,observed,disconnected:false,observe:node=>observed.push(node),disconnect(){this.disconnected=true;}};observers.push(value);return value;}});
 const nodes=Array.from({length:4},(_,i)=>({dataset:{clip:String(i)},isConnected:true}));
 const rail={querySelectorAll:()=>nodes};
 const entry=(node,shown=true)=>({target:node,isIntersecting:shown,intersectionRect:{width:100,height:shown?100:0}});
 return {tracker,observers,nodes,rail,entry,updates:()=>updates};
}
test('visibility changes return visible IDs in catalog order without duplicate scheduling',()=>{
 const f=fixture();f.tracker.bind(f.rail);const o=f.observers[0];
 assert.equal(o.options.root,null);assert.equal(o.observed.length,4);
 o.callback([f.entry(f.nodes[2]),f.entry(f.nodes[0])]);
 assert.deepEqual(f.tracker.ids(),['0','2']);assert.equal(f.updates(),1);
 o.callback([f.entry(f.nodes[0])]);assert.equal(f.updates(),1);
 o.callback([f.entry(f.nodes[0],false)]);assert.deepEqual(f.tracker.ids(),['2']);
});
test('detached cards are ignored while touching-edge entries prefetch before a missed threshold-zero transition',()=>{
 const f=fixture();f.tracker.bind(f.rail);const o=f.observers[0];
 f.nodes[0].isConnected=false;
 o.callback([f.entry(f.nodes[0]),{...f.entry(f.nodes[1]),intersectionRect:{width:0,height:100}}]);
 assert.deepEqual(f.tracker.ids(),['1']);assert.equal(f.updates(),1);
 // No second notification is required when this edge moves inside the root.
 assert.deepEqual(f.tracker.ids(),['1']);
});
test('replacing a rail disconnects and ignores queued callbacks from its old generation',()=>{
 const f=fixture();f.tracker.bind(f.rail);const old=f.observers[0];
 old.callback([f.entry(f.nodes[0])]);f.tracker.bind(f.rail);
 assert.ok(old.disconnected);assert.deepEqual(f.tracker.ids(),[]);
 old.callback([f.entry(f.nodes[1])]);assert.deepEqual(f.tracker.ids(),[]);assert.equal(f.updates(),1);
 f.observers[1].callback([f.entry(f.nodes[3])]);assert.deepEqual(f.tracker.ids(),['3']);
});
test('leaving Replay releases observations and rejects late visibility updates',()=>{
 const f=fixture();f.tracker.bind(f.rail);const old=f.observers[0];f.tracker.bind(null);
 old.callback([f.entry(f.nodes[1])]);assert.ok(old.disconnected);assert.deepEqual(f.tracker.ids(),[]);assert.equal(f.updates(),0);
});
