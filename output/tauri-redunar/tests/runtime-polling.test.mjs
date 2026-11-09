import {test} from 'node:test';
import assert from 'node:assert/strict';
import {startRuntime, latestRuntimeRefresh, pollVisibleRuntime} from '../ui/runtime-polling.mjs';

test('polling starts while a startup update remains pending',async()=>{
 let resolveUpdate, polls=0, rendered=0, updates=0;
 const update=new Promise(resolve=>{resolveUpdate=resolve;});
 await startRuntime({initialize:async()=>{},refresh:async()=>{},render:()=>rendered++,checkUpdates:()=>update,updatePresentation:()=>updates++,startPolling:()=>polls++,onError:assert.fail});
 assert.equal(polls,1);assert.equal(rendered,1);
 resolveUpdate();await new Promise(resolve=>setImmediate(resolve));
 assert.equal(rendered,1);assert.equal(updates,1);
});
test('a failed update leaves polling running and reports once',async()=>{
 let polls=0;const errors=[];
 await startRuntime({initialize:async()=>{},refresh:async()=>{},render:()=>{},checkUpdates:async()=>{throw Error('offline');},updatePresentation:()=>assert.fail('failed update has no presentation result'),startPolling:()=>polls++,onError:error=>errors.push(error.message)});
 await new Promise(resolve=>setImmediate(resolve));
 assert.equal(polls,1);assert.deepEqual(errors,['offline']);
});
test('concurrent refreshes retain one latest follow-up and recover after failure',async()=>{
 let release, calls=0, active=0, peak=0;
 const refresh=latestRuntimeRefresh(async()=>{calls++;peak=Math.max(peak,++active);try{if(calls===1)await new Promise(resolve=>{release=resolve;});if(calls===3)throw Error('read failed');}finally{active--;}});
 const first=refresh();await Promise.resolve();
 for(let i=0;i<20;i++)assert.equal(refresh(),first);
 release();await first;assert.equal(calls,2);assert.equal(peak,1);
 await assert.rejects(refresh(),/read failed/);await refresh();assert.equal(calls,4);
});

test('hidden runtime polling stops and reopening reads once even during a slow refresh',async(t)=>{
 t.mock.timers.enable({apis:['setTimeout']});
 const page=new EventTarget();page.hidden=false;
 let reads=0,release;
 pollVisibleRuntime(page,async()=>{reads++;if(reads===1)await new Promise(resolve=>{release=resolve;});},()=>1000,assert.fail);
 const visible=value=>{page.hidden=!value;page.dispatchEvent(new Event('visibilitychange'));};
 visible(false);
 t.mock.timers.tick(5000);assert.equal(reads,0);
 visible(true);t.mock.timers.tick(0);assert.equal(reads,1);
 visible(false);visible(true);t.mock.timers.tick(5000);assert.equal(reads,1);
 release();await new Promise(resolve=>setImmediate(resolve));
 t.mock.timers.tick(999);assert.equal(reads,1);
 t.mock.timers.tick(1);assert.equal(reads,2);
 visible(false);await new Promise(resolve=>setImmediate(resolve));
 t.mock.timers.tick(5000);assert.equal(reads,2);
});
