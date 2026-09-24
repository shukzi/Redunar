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
