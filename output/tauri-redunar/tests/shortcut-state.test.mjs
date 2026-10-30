import {test} from 'node:test';
import assert from 'node:assert/strict';
import {canRetryShortcuts} from '../ui/shortcut-state.mjs';

test('saved shortcuts can retry failed activation without a configuration write',()=>{
  for(const state of ['Inactive','Unavailable','Denied'])assert.equal(canRetryShortcuts({state},['','Alt+Z']),true);
  for(const state of ['Active','Starting'])assert.equal(canRetryShortcuts({state},['Alt+Z']),false);
  assert.equal(canRetryShortcuts({state:'Unavailable'},['',' ']),false);
});
