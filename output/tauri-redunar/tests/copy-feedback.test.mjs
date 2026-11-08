import test from 'node:test';
import assert from 'node:assert/strict';
import {createCopyFeedback} from '../ui/copy-feedback.mjs';

function fixture() {
  const jobs=new Map(),paint=[];
  let serial=0;
  const button={disabled:false,isConnected:true,setAttribute(){},removeAttribute(){}};
  const copy=createCopyFeedback({render:(node,copied)=>paint.push(copied),setTimer:(fn,ms)=>{assert.equal(ms,3000);jobs.set(++serial,fn);return serial;},clearTimer:id=>jobs.delete(id)});
  return {button,copy,jobs,paint};
}
test('Copy confirms only after writing and ignores duplicate pending clicks',async()=>{
  const f=fixture();let finish,writes=0;
  const pending=f.copy(f.button,()=>{writes++;return new Promise(resolve=>finish=resolve);});
  assert.equal(f.button.disabled,true);assert.deepEqual(f.paint,[]);
  assert.equal(await f.copy(f.button,()=>writes++),false);assert.equal(writes,1);
  finish();assert.equal(await pending,true);
  assert.equal(f.button.disabled,false);assert.deepEqual(f.paint,[true]);
  [...f.jobs.values()][0]();assert.deepEqual(f.paint,[true,false]);
});
test('Repeated success restarts feedback; a rejection restores Copy and permits retry',async()=>{
  const f=fixture();
  await f.copy(f.button,async()=>{});
  const first=[...f.jobs.keys()][0];
  await f.copy(f.button,async()=>{});
  assert.equal(f.jobs.has(first),false);assert.equal(f.jobs.size,1);
  await assert.rejects(f.copy(f.button,async()=>{throw Error('Clipboard denied');}),/Clipboard denied/);
  assert.equal(f.jobs.size,0);assert.equal(f.button.disabled,false);assert.equal(f.paint.at(-1),false);
  await f.copy(f.button,async()=>{});assert.equal(f.paint.at(-1),true);
});
test('A late clipboard result cannot repaint a detached button',async()=>{
  const f=fixture();let finish;
  const pending=f.copy(f.button,()=>new Promise(resolve=>finish=resolve));
  f.button.isConnected=false;finish();await pending;
  assert.deepEqual(f.paint,[]);assert.equal(f.jobs.size,0);
});
