import {test} from 'node:test';
import assert from 'node:assert/strict';
import {startRuntime, latestRuntimeRefresh} from '../ui/runtime-polling.mjs';

test('polling starts while a startup update remains pending',async()=>{
 let resolveUpdate, polls=0, rendered=0;
 const update=new Promise(resolve=>{resolveUpdate=resolve;});
 await startRuntime({initialize:async()=>{},refresh:async()=>{},render:()=>rendered++,checkUpdates:()=>update,startPolling:()=>polls++,onError:assert.fail});
 assert.equal(polls,1);assert.equal(rendered,1);
 resolveUpdate();await new Promise(resolve=>setImmediate(resolve));
 assert.equal(rendered,2);
});
test('a failed update leaves polling running and reports once',async()=>{
 let polls=0;const errors=[];
 await startRuntime({initialize:async()=>{},refresh:async()=>{},render:()=>{},checkUpdates:async()=>{throw Error('offline');},startPolling:()=>polls++,onError:error=>errors.push(error.message)});
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
