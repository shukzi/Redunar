import {test} from 'node:test';
import assert from 'node:assert/strict';
import {toastStore} from '../ui/toast-store.mjs';
function fixture(){
  let time=0,serial=0;const timers=new Map();
  const store=toastStore({now:()=>time,setTimer:(fn,delay)=>{const id=++serial;timers.set(id,{fn,at:time+delay});return id;},clearTimer:id=>timers.delete(id)});
  return {store,timers,advance(ms){time+=ms;for(const [id,timer] of timers)if(timer.at<=time){timers.delete(id);timer.fn();}}};
}
test('repeated messages update one notification and stale expiry cannot dismiss it',()=>{
  const {store,timers,advance}=fixture();
  const id=store.add('Saved',{duration:5000}),stale=[...timers.values()][0].fn;
  advance(4000);assert.equal(store.add('Saved',{duration:5000}),id);
  stale();assert.equal(store.snapshot().length,1);
  advance(4999);assert.equal(store.snapshot().length,1);
  advance(1);assert.equal(store.snapshot().length,0);
});
test('hover, keyboard focus and background pauses preserve the remaining reading time',()=>{
  const {store,advance}=fixture();
  store.add('Keep this message',{duration:5000});advance(1000);
  store.pause('hover');store.pause('focus');advance(20000);
  assert.equal(store.snapshot()[0].remaining,4000);
  store.resume('hover');advance(5000);assert.equal(store.snapshot().length,1);
  store.resume('focus');advance(3999);assert.equal(store.snapshot().length,1);
  advance(1);assert.equal(store.snapshot().length,0);
});
test('bursts keep the newest three and removed timers cannot affect another toast',()=>{
  const {store,timers}=fixture();
  store.add('one');const stale=[...timers.values()][0].fn;
  const second=store.add('two');store.add('three');store.add('four');
  assert.deepEqual(store.snapshot().map(entry=>entry.message),['two','three','four']);
  stale();assert.equal(store.snapshot().length,3);
  store.remove(second);assert.deepEqual(store.snapshot().map(entry=>entry.message),['three','four']);
  store.dispose();assert.equal(timers.size,0);assert.equal(store.snapshot().length,0);
});
test('messages added while paused get their whole reading time after resume',()=>{
  const {store,timers,advance}=fixture();
  store.pause('hidden');store.add('new',{duration:5000});advance(20000);
  assert.equal(timers.size,0);assert.equal(store.snapshot()[0].remaining,5000);
  store.resume('hidden');advance(4999);assert.equal(store.snapshot().length,1);
  advance(1);assert.equal(store.snapshot().length,0);
});
