import test from 'node:test';
import assert from 'node:assert/strict';
import {installIconMotion} from '../ui/icon-motion.mjs';
import {icon} from '../ui/icons.mjs';

test('Hover and focus share one gesture; reduced motion cancels it and suppresses replays',async()=>{
  const handlers={},preference={matches:false,addEventListener:(type,fn)=>handlers.preference=fn};
  const view={matchMedia:()=>preference,addEventListener:(type,fn)=>handlers[type]=fn};
  const doc={defaultView:view,addEventListener:(type,fn)=>handlers[type]=fn};
  let starts=0,cancels=0,finish;
  const group={ownerDocument:doc,children:[],style:{},animate(){
    starts++;const finished=new Promise(resolve=>finish=resolve);
    return {finished,cancel(){cancels++;finish();}};
  }};
  const svg={ownerDocument:doc,dataset:{lucide:'refresh-cw'},classList:{contains:()=>false},querySelector:()=>group};
  const control={matches:selector=>selector.includes('#navigation .nav-link'),closest:()=>null,contains:()=>false,querySelector:selector=>selector.startsWith('.copy-')?null:svg};
  const target={closest:()=>control,matches:()=>true};
  installIconMotion(doc);
  handlers.pointerover({type:'pointerover',target,relatedTarget:null});
  handlers.focusin({type:'focusin',target,relatedTarget:null});
  handlers.pointerover({type:'pointerover',target,relatedTarget:null});
  assert.equal(starts,1);
  preference.matches=true;handlers.preference();
  assert.equal(cancels,1);
  await Promise.resolve();
  handlers.pointerover({type:'pointerover',target,relatedTarget:null});
  assert.equal(starts,1);
  handlers.pagehide({persisted:false});
});

test('Overview uses Group; the reference motions keep the fixed Library books still',()=>{
  assert.match(icon('overview'),/data-lucide="group"/);
  assert.match(icon('overview'),/class="group-frame"/);
  function gesture(kind,count){
    const handlers={},motions=[];
    const preference={matches:false,addEventListener(){}};
    const doc={defaultView:{matchMedia:()=>preference,addEventListener(){}},addEventListener:(type,fn)=>handlers[type]=fn};
    const children=Array.from({length:count},(_,index)=>({style:{},ownerDocument:doc,animate(frames,options){motions.push({index,frames,options});return {finished:new Promise(()=>{}),cancel(){}};}}));
    const svg={ownerDocument:doc,dataset:{lucide:kind},classList:{contains:()=>false},querySelector:()=>({children})};
    const control={matches:selector=>selector.includes('#navigation .nav-link'),closest:()=>null,contains:()=>false,querySelector:selector=>selector.startsWith('.copy-')?null:svg};
    installIconMotion(doc);handlers.pointerover({type:'pointerover',target:{closest:()=>control},relatedTarget:null});
    return {motions,children};
  }
  const library=gesture('library-big',3);
  assert.deepEqual(library.motions.map(motion=>motion.index),[2]);
  assert.equal(library.motions[0].frames[1].transform,'rotate(20deg)');
  assert.equal(library.children[2].style.transformOrigin,'17.6px 20.96px');
  const group=gesture('group',3);
  assert.equal(group.motions.length,3);
  assert.equal(group.motions[0].frames[1].transform,'scale(.85)');
  assert.match(group.motions[1].frames[1].transform,/0.75px,0.75px/);
  assert.match(group.motions[2].frames[1].transform,/-0.75px,-0.75px/);
});

test('Cancelling Trash motion restores every path and releases its frame',async()=>{
  const raf=globalThis.requestAnimationFrame,caf=globalThis.cancelAnimationFrame,frames=new Map();let next=0;
  globalThis.requestAnimationFrame=callback=>{frames.set(++next,callback);return next;};
  globalThis.cancelAnimationFrame=id=>frames.delete(id);
  const handlers={},preference={matches:false,addEventListener:(type,fn)=>handlers.preference=fn};
  const doc={defaultView:{matchMedia:()=>preference,addEventListener:(type,fn)=>handlers[type]=fn},addEventListener:(type,fn)=>handlers[type]=fn};
  const children=Array.from({length:5},(_,i)=>({d:`original-${i}`,style:{transform:''},getAttribute(){return this.d;},setAttribute(name,value){this.d=value;}}));
  const svg={ownerDocument:doc,dataset:{lucide:'trash'},classList:{contains:()=>false},querySelector:()=>({children})};
  const control={matches:selector=>selector.includes('#navigation .nav-link'),closest:()=>null,contains:()=>false,querySelector:selector=>selector.startsWith('.copy-')?null:svg};
  try{
    installIconMotion(doc);handlers.pointerover({type:'pointerover',target:{closest:()=>control},relatedTarget:null});
    assert.equal(frames.size,1);
    const [id,frame]=frames.entries().next().value;frames.delete(id);frame(performance.now()+350);
    assert.notEqual(children[1].d,'original-1');
    assert.notEqual(children[0].style.transform,children[1].style.transform);
    preference.matches=true;handlers.preference();
    assert.deepEqual(children.map(part=>part.d),children.map((_,i)=>`original-${i}`));
    assert.ok(children.every(part=>part.style.transform===''));
    assert.equal(frames.size,0);
    await Promise.resolve();handlers.pagehide({persisted:false});
  }finally{
    globalThis.requestAnimationFrame=raf;globalThis.cancelAnimationFrame=caf;
  }
});
