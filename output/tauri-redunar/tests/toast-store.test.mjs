import {test} from 'node:test';
import assert from 'node:assert/strict';
import {toastStore} from '../ui/toast-store.mjs';
function fixture(){
  let time=0,wall=0,serial=0;const timers=new Map();
  const store=toastStore({now:()=>time,wallNow:()=>wall,setTimer:(fn,delay)=>{const id=++serial;timers.set(id,{fn,at:time+delay});return id;},clearTimer:id=>timers.delete(id)});
  return {store,timers,elapse(ms,{deliver=true,monotonic=ms,wallClock=ms}={}){
    time+=monotonic;wall+=wallClock;
    if(deliver)for(const [id,timer] of [...timers])if(timer.at<=time){timers.delete(id);timer.fn();}
  }};
}
test('routine confirmations expire quickly and longer failures have bounded reading time',()=>{
  const {store,elapse}=fixture();
  store.add('Saved',{type:'success'});assert.equal(store.snapshot()[0].duration,2000);
  elapse(1999);assert.equal(store.snapshot().length,1);elapse(1);assert.deepEqual(store.snapshot(),[]);
  store.add('Updated');assert.equal(store.snapshot()[0].duration,2500);
  store.add('Check the folder',{type:'warning'});assert.equal(store.snapshot()[0].duration,4000);
  store.add('Cannot save',{type:'error'});assert.equal(store.snapshot()[0].duration,5000);
  store.add('Long error '.repeat(200),{type:'error'});assert.equal(store.snapshot()[0].duration,6000);
  elapse(6000);assert.deepEqual(store.snapshot(),[]);
});
test('repeated messages reuse one entry and stale expiry cannot dismiss the refreshed message',()=>{
  const {store,timers,elapse}=fixture();
  const id=store.add('Saved'),stale=[...timers.values()][0].fn;
  elapse(2000);assert.equal(store.add('Saved'),id);assert.equal(timers.size,1);
  stale();assert.equal(store.snapshot().length,1);
  elapse(2499);assert.equal(store.snapshot().length,1);elapse(1);assert.equal(store.snapshot().length,0);
});
test('bursts keep one message and one timer without replaying older messages',()=>{
  const {store,timers,elapse}=fixture();
  const first=store.add('first'),stale=[...timers.values()][0].fn;
  for(let i=0;i<1000;i++)store.add(String(i));
  assert.deepEqual(store.snapshot().map(entry=>entry.message),['999']);assert.equal(timers.size,1);
  stale();store.remove(first);assert.equal(store.snapshot()[0].message,'999');
  elapse(2500);assert.equal(timers.size,0);assert.deepEqual(store.snapshot(),[]);
});
test('delayed timer delivery cannot keep an expired message after a foreground recheck',()=>{
  const {store,timers,elapse}=fixture();
  store.add('Saved');const stale=[...timers.values()][0].fn;
  elapse(20000,{deliver:false});store.expire();
  assert.deepEqual(store.snapshot(),[]);assert.equal(timers.size,0);
  store.add('Fresh');stale();assert.equal(store.snapshot()[0].message,'Fresh');
});
test('reading can defer removal but never extend the deadline when backgrounded',()=>{
  const {store,timers,elapse}=fixture();
  store.add('Saved');elapse(1000);const deadline=store.snapshot()[0].deadline;
  store.hold(true);elapse(20000);
  assert.equal(store.snapshot().length,1);assert.equal(timers.size,0);
  assert.equal(store.snapshot()[0].deadline,deadline);
  store.hold(false);assert.deepEqual(store.snapshot(),[]);assert.equal(timers.size,0);
});
test('leaving a reading hold before the deadline preserves only the actual remaining time',()=>{
  const {store,timers,elapse}=fixture();
  store.add('Saved');store.hold(true);elapse(1000);store.hold(false);
  assert.equal(store.snapshot()[0].remaining,1500);assert.equal(timers.size,1);
  elapse(1500);assert.deepEqual(store.snapshot(),[]);
});
test('sleep expiry uses wall time and backward clock changes cannot extend monotonic expiry',()=>{
  const {store,elapse}=fixture();
  store.add('Before sleep');elapse(20000,{monotonic:0,deliver:false});store.expire();
  assert.deepEqual(store.snapshot(),[]);
  store.add('Before clock change');elapse(2500,{wallClock:-60000});
  assert.deepEqual(store.snapshot(),[]);
});
test('early callbacks rearm for the remaining interval without expiring a current message',()=>{
  const {store,timers,elapse}=fixture();
  store.add('Saved');elapse(1000,{deliver:false});
  const [id,early]=[...timers][0];timers.delete(id);early.fn();
  assert.equal(store.snapshot()[0].remaining,1500);assert.equal(timers.size,1);
  elapse(1500);assert.deepEqual(store.snapshot(),[]);
});
test('invalid durations and types cannot create immortal notifications',()=>{
  const {store,elapse}=fixture();
  for(const duration of [NaN,Infinity,-Infinity,null,'forever',-1,0,1e100]){
    store.add('Finite',{duration,type:'unknown'});
    const entry=store.snapshot()[0];assert.equal(entry.type,'info');
    assert.ok(entry.duration>=1000&&entry.duration<=6000);elapse(6000);assert.deepEqual(store.snapshot(),[]);
  }
  assert.equal(store.add('  '),undefined);assert.equal(store.add(null),undefined);
});
test('disposal is idempotent and stale callbacks or retained owners cannot restart work',()=>{
  const {store,timers}=fixture();
  store.add('Saved');const stale=[...timers.values()][0].fn;
  store.dispose();store.dispose();store.hold(true);stale();
  assert.equal(store.add('Late result'),undefined);assert.equal(timers.size,0);assert.deepEqual(store.snapshot(),[]);
});
