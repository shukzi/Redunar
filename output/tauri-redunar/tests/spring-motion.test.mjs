import test from 'node:test';
import assert from 'node:assert/strict';
import {springMotion,springStep} from '../ui/spring-motion.mjs';

function fixture(){
  let time=0,serial=0;const frames=new Map(),paint=[];
  const motion=springMotion({x:0},value=>paint.push(value.x),{now:()=>time,requestFrame:fn=>{frames.set(++serial,fn);return serial;},cancelFrame:id=>frames.delete(id)});
  const tick=ms=>{time+=ms;const callbacks=[...frames.values()];frames.clear();callbacks.forEach(fn=>fn(time));};
  return {motion,frames,paint,tick};
}
test('The same elapsed time gives the same spring position across refresh rates',()=>{
  const whole=springStep(0,0,200,.1);let state={position:0,velocity:0};
  for(let i=0;i<12;i++)state=springStep(state.position,state.velocity,200,1/120);
  assert.ok(Math.abs(whole.position-state.position)<1e-9);
  assert.ok(Math.abs(whole.velocity-state.velocity)<1e-9);
});
test('Rapid retargeting preserves painted position and velocity, with one queued frame',()=>{
  const f=fixture();f.motion.to({x:200});f.tick(50);
  const before=f.motion.snapshot();f.motion.to({x:40});const after=f.motion.snapshot();
  assert.deepEqual(before.value,after.value);assert.deepEqual(before.velocity,after.velocity);
  f.motion.to({x:40});assert.equal(f.frames.size,1);
  for(let i=0;i<80&&f.frames.size;i++)f.tick(16);
  assert.equal(f.motion.running,false);assert.equal(f.paint.at(-1),40);assert.equal(f.frames.size,0);
});
test('Binding a replacement surface retains the active motion',()=>{
  const f=fixture();f.motion.to({x:200});f.tick(70);const position=f.motion.snapshot().value.x,newPaint=[];
  f.motion.bind(value=>newPaint.push(value.x));f.motion.to({x:200});
  assert.equal(newPaint[0],position);assert.equal(f.frames.size,1);
  for(let i=0;i<80&&f.frames.size;i++)f.tick(16);
  assert.equal(newPaint.at(-1),200);
});
test('An immediate reduced-motion update invalidates an already queued frame',()=>{
  const f=fixture();f.motion.to({x:200});const stale=[...f.frames.values()][0];f.tick(20);
  f.motion.to({x:40},{immediate:true});const count=f.paint.length;
  stale(500);assert.equal(f.paint.length,count);assert.equal(f.paint.at(-1),40);assert.equal(f.frames.size,0);
});
