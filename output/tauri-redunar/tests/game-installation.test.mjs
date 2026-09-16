import test from 'node:test';
import assert from 'node:assert/strict';
import {gameInstallation} from '../ui/game-installation.js';

test('installation errors disable launch until a successful refresh', async () => {
  let fail=false, status='installed', changes=0;
  const state=gameInstallation(async command=>{
    assert.equal(command,'game_installation_statuses');
    if(fail)throw Error('unreadable library');
    return {'1':status};
  },()=>changes++);
  assert.equal(state.status('1'),'checking');
  await state.refresh();assert.equal(state.status('1'),'installed');
  status='not_installed';await state.refresh();assert.equal(state.status('1'),'not_installed');
  fail=true;await state.refresh();assert.equal(state.status('1'),'unknown');
  fail=false;status='installed';await state.refresh();assert.equal(state.status('1'),'installed');
  assert.equal(changes,4);
});

test('overlapping refreshes queue one latest check', async () => {
  const pending=[];
  const state=gameInstallation(()=>new Promise(resolve=>pending.push(resolve)),()=>{});
  const first=state.refresh();state.refresh();state.refresh();
  assert.equal(pending.length,1);
  pending.shift()({'1':'installed'});
  await Promise.resolve();
  assert.equal(pending.length,1);
  pending.shift()({'1':'not_installed','2':'installed'});
  await first;
  assert.equal(state.status('1'),'not_installed');
  assert.equal(state.status('2'),'installed');
  assert.equal(pending.length,0);
});
