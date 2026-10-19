import test from 'node:test';
import assert from 'node:assert/strict';
import {playbackLevel, playbackVolume} from '../ui/playback-volume.mjs';
import {microphoneTestClient} from '../ui/microphone-controls.mjs';

const deferred=()=>{let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};};
const tick=()=>new Promise(resolve=>setImmediate(resolve));

test('listening preference affects only player volume and survives restricted storage',()=>{
  const player={volume:1,muted:true,exportSettings:{audio:'original'}};
  const preferences=playbackVolume({storage:{getItem:()=> '35'},getVideo:()=>player});
  preferences.apply(player);
  assert.equal(player.volume,.35);assert.equal(player.muted,false);
  assert.deepEqual(player.exportSettings,{audio:'original'});
  const blocked=playbackVolume({storage:{getItem:()=>{throw Error('blocked');}},getVideo:()=>player});
  blocked.apply(player);assert.equal(player.volume,1);
  for(const value of [null,'35',NaN,Infinity,{}])assert.equal(playbackLevel(value),100);
  const muted=playbackVolume({storage:{getItem:()=>JSON.stringify({level:0,previous:35})},getVideo:()=>player});
  muted.apply(player);assert.equal(player.volume,0);
  assert.equal(playbackLevel(-5),0);assert.equal(playbackLevel(150),100);
});

test('cancel during pending test start reaps the returned native identity',async()=>{
  const pending=deferred(),calls=[],states=[];
  const client=microphoneTestClient({call:async(command,args)=>{calls.push([command,args]);if(command==='start_microphone_test')return pending.promise;},changed:state=>states.push(state)});
  const started=client.start();await tick();await client.cancel();
  pending.resolve({id:19,phase:'recording'});await started;
  assert.deepEqual(calls.at(-1),['stop_microphone_test',{id:19}]);
  assert.deepEqual(states.map(s=>s.phase),['starting']);
});

test('cancel during test audio read prevents stale playback',async()=>{
  const audio=deferred(),states=[];
  const client=microphoneTestClient({call:async(command)=>{if(command==='start_microphone_test')return {id:20,phase:'ready'};if(command==='microphone_test_audio')return audio.promise;},changed:state=>states.push(state)});
  const started=client.start();await tick();await client.cancel();audio.resolve([1,2,3]);await started;
  assert.equal(states.some(state=>state.bytes),false);
});

test('polls remain serial and capture failure is explicit',async()=>{
  const callbacks=[],states=[];
  let polls=0;
  const client=microphoneTestClient({call:async(command)=>{if(command==='start_microphone_test')return {id:21,phase:'recording'};if(command==='microphone_test_status'){polls++;throw Error('Input disconnected');}},changed:state=>states.push(state),schedule:callback=>{callbacks.push(callback);return 1;},unschedule:()=>{}});
  await client.start();assert.equal(callbacks.length,1);callbacks.shift()();await tick();
  assert.equal(polls,1);assert.equal(callbacks.length,0);assert.equal(states.at(-1).phase,'failed');assert.match(states.at(-1).message,/Input disconnected/);
  await client.cancel();
});
