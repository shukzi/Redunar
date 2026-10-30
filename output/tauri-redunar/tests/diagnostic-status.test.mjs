import test from 'node:test';
import assert from 'node:assert/strict';
import { diagnosticStatus } from '../ui/diagnostic-status.mjs';
test('saved preference never fabricates active logging',()=>{
 assert.match(diagnosticStatus('off',true),/Restart/);
 assert.match(diagnosticStatus('active',false),/until you restart/);
 assert.match(diagnosticStatus('unavailable',true),/unavailable/);
 assert.match(diagnosticStatus(undefined,true),/unavailable/);
});
