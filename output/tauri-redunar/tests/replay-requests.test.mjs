import {test} from 'node:test';
import assert from 'node:assert/strict';
import {latestReplayRequest} from '../ui/replay-requests.mjs';

const tick = () => new Promise(resolve=>setImmediate(resolve));
function fixture() {
  const started = [], accepted = [], rejected = [], jobs = [], cancellations = [];
  const queue = latestReplayRequest(value=>{
    started.push(value);
    return new Promise((resolve,reject)=>jobs.push({resolve,reject}));
  },()=>cancellations.push(started.at(-1)));
  return {queue, started, accepted, rejected, jobs, cancellations,
    select: value=>queue.request(value,result=>accepted.push(result),error=>rejected.push(error)),
  };
}

test('rapid selection prepares one clip at a time and skips intermediate clips',async()=>{
  const f=fixture();
  f.select('a');f.select('b');f.select('c');
  assert.deepEqual(f.started,['a']);
  assert.deepEqual(f.cancellations,['a']);
  f.jobs[0].resolve('obsolete URL');await tick();
  assert.deepEqual(f.accepted,[]);
  assert.deepEqual(f.started,['a','c']);
  f.jobs[1].resolve('current URL');await tick();
  assert.deepEqual(f.accepted,['current URL']);
});

test('leaving Replay discards queued work and ignores a stale failure',async()=>{
  const f=fixture();
  f.select('a');f.select('b');f.queue.invalidate();
  assert.deepEqual(f.cancellations,['a']);
  f.jobs[0].reject('obsolete failure');await tick();
  assert.deepEqual(f.started,['a']);
  assert.deepEqual(f.rejected,[]);
  f.select('c');f.jobs[1].resolve('returned to Replay');await tick();
  assert.deepEqual(f.accepted,['returned to Replay']);
});

test('a failed preparation reports once and allows the next selection',async()=>{
  const f=fixture();
  f.select('a');f.jobs[0].reject('unavailable');await tick();
  assert.deepEqual(f.rejected,['unavailable']);
  f.select('b');f.jobs[1].resolve('ready');await tick();
  assert.deepEqual(f.accepted,['ready']);
});
