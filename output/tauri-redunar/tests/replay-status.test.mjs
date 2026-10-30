import assert from 'node:assert/strict';
import test from 'node:test';
import { replayStatusCopy } from '../ui/replay-menu-view.mjs';

test('a native source rejection remains visible while the recorder is inactive', () => {
  const reason = 'This OpenGL driver cannot share Replay frames with the encoder. Check graphics driver support.';
  assert.deepEqual(replayStatusCopy({ phase: 'Inactive', unavailable_reason: reason }), {
    title: 'Replay unavailable',
    detail: reason,
    statusClass: 'error',
  });
});

test('healthy buffering does not reuse an earlier unavailable reason', () => {
  const status = replayStatusCopy({ phase: 'Buffering', buffered_seconds: 3 });
  assert.equal(status.title, 'Buffering replay');
  assert.equal(status.statusClass, 'pending');
});

test('waiting for frames is pending and does not imply recording is ready', () => {
  const detail = 'Waiting for recordable game frames.';
  assert.deepEqual(replayStatusCopy({phase:'Inactive',pending_reason:detail,can_save:false}), {
    title:'Waiting for Replay frames',detail,statusClass:'pending',
  });
});

test('explicit encoder failure or source rejection takes priority over pending text', () => {
  for (const field of ['failure','unavailable_reason']) {
    const detail = field === 'failure' ? 'The video encoder failed.' : 'Resolution unsupported.';
    const status = replayStatusCopy({phase:'Inactive',pending_reason:'Waiting for frames.',[field]:detail});
    assert.equal(status.statusClass,'error');
    assert.equal(status.detail,detail);
  }
});

test('idle and healthy states ignore stale pending-frame copy', () => {
  assert.equal(replayStatusCopy({phase:'Inactive'}).title,'Replay inactive');
  const status = replayStatusCopy({phase:'Buffering',can_save:true,buffered_seconds:3,pending_reason:'Waiting for frames.'});
  assert.equal(status.title,'Ready to save');
  assert.equal(status.statusClass,'ready');
});
