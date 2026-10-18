import {test} from 'node:test';
import assert from 'node:assert/strict';

let fixtureId=0;
const flush=async()=>{for(let i=0;i<8;i++)await new Promise(resolve=>setImmediate(resolve));};
async function fixture(ids) {
 const images=[];
 globalThis.document={createElement:()=>{const image={events:{},addEventListener(name,fn){this.events[name]=fn;}};images.push(image);return image;}};
 const node=id=>({dataset:{poster:String(id)},childNodes:[{label:'fallback',cloneNode(){return {...this};}}],hasAttribute:()=>false,
  querySelector(){return this.children?.find(c=>c.events)||null;},replaceChildren(...children){this.children=children;}});
 const root={isConnected:true,nodes:ids.map(node),queries:0,querySelectorAll(){this.queries++;return this.nodes;}};
 const {loadGameArtwork}=await import(`../ui/game-artwork.js?fixture=${++fixtureId}`);
 return {root,images,node,load:call=>loadGameArtwork(root,call)};
}

test('warm artwork paints every matching node without rescanning or refetching',async()=>{
 const f=await fixture([1,1,2]);let calls=0;
 const call=async()=>{calls++;return [255,216,255];};
 f.load(call);await flush();assert.equal(calls,2);
 f.root.nodes=[f.node(1),f.node(1),f.node(2)];f.root.queries=0;
 f.load(call);await flush();
 assert.equal(calls,2,'cached artwork must not trigger native work');
 assert.equal(f.root.queries,1,'one catalog pass, independent of cached key count');
 assert.ok(f.root.nodes.every(n=>n.querySelector('img')));
 const image=f.root.nodes[0].querySelector('img');f.load(call);
 assert.equal(f.root.nodes[0].querySelector('img'),image,'retain mounted images');
});

test('failed image decoding restores its own fallback and retains a cached miss',async()=>{
 const f=await fixture([1]);let calls=0;
 f.load(async()=>{calls++;return [255,216,255];});await flush();
 f.images[0].events.error();
 assert.equal(f.root.nodes[0].children[0].label,'fallback');
 f.root.nodes=[f.node(1)];f.load(async()=>{calls++;return [255,216,255];});await flush();
 assert.equal(calls,1);assert.equal(f.root.nodes[0].querySelector('img'),null);
});

test('native requests remain bounded and detached results only warm the cache',async()=>{
 const f=await fixture([1,2,3]);const pending=[];
 const call=()=>new Promise(resolve=>pending.push(resolve));
 f.load(call);assert.equal(pending.length,2);
 f.root.isConnected=false;pending.forEach(resolve=>resolve([255,216,255]));await flush();
 assert.equal(f.images.length,0,'do not paint detached content');
 f.root.isConnected=true;f.load(call);await flush();
 assert.equal(f.images.length,2,'completed results remain usable');
 assert.equal(pending.length,3,'unfinished node can still be requested');
 pending[2]([255,216,255]);await flush();
 assert.equal(f.images.length,3);
});
