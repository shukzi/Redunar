import test from 'node:test';
import assert from 'node:assert/strict';
import {nextChoiceIndex} from '../ui/choice-switchers.mjs';

test('short-choice keyboard navigation wraps and skips disabled values',()=>{
 const disabled=[false,true,false,false];
 assert.equal(nextChoiceIndex(disabled,0,'ArrowRight'),2);
 assert.equal(nextChoiceIndex(disabled,0,'ArrowLeft'),3);
 assert.equal(nextChoiceIndex(disabled,2,'Home'),0);
 assert.equal(nextChoiceIndex(disabled,2,'End'),3);
 assert.equal(nextChoiceIndex(disabled,0,'Enter'),null);
 assert.equal(nextChoiceIndex([true,true],0,'ArrowRight'),-1);
 assert.equal(nextChoiceIndex([],0,'Home'),-1);
});

test('vertical keys also move through enabled setting choices',()=>{
 assert.equal(nextChoiceIndex([false,false,false,false],0,'ArrowDown'),1);
 assert.equal(nextChoiceIndex([false,false,false,false],0,'ArrowUp'),3);
 assert.equal(nextChoiceIndex([false,true,false,false],0,'ArrowDown'),2);
});
