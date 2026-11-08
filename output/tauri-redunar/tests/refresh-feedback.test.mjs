import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRefreshFeedback} from '../ui/refresh-feedback.mjs';

function fixture(){
  let time=0,serial=0;
  const timers=new Map(),buttons=['history','inputs'].map(key=>({
    dataset:{refreshKey:key},attributes:{},symbol:{},status:{},label:{textContent:key==='inputs'?'Refresh inputs':'Refresh history'},
    setAttribute(name,value){this.attributes[name]=value;},
    removeAttribute(name){delete this.attributes[name];},
    querySelector(selector){return selector==='[data-refresh-symbol]'?this.symbol:selector==='.refresh-status'?this.status:this.label;},
  }));
  const root={querySelectorAll:()=>buttons};
  const feedback=createRefreshFeedback({changed:()=>feedback.sync(root),now:()=>time,
    schedule:(callback,delay)=>{timers.set(++serial,{at:time+delay,callback});return serial;},
    unschedule:id=>timers.delete(id)});
  const advance=duration=>{
    const end=time+duration;
    for(;;){const entry=[...timers].filter(([,timer])=>timer.at<=end).sort((a,b)=>a[1].at-b[1].at)[0];if(!entry)break;timers.delete(entry[0]);time=entry[1].at;entry[1].callback();}
    time=end;
  };
  feedback.sync(root);
  return {feedback,buttons,advance,timers};
}

test('one read owns progress, duplicate requests and its bounded acknowledgement',async()=>{
  const {feedback,buttons,advance,timers}=fixture();let resolve,calls=0;
  const pending=feedback.run('history',()=>{calls++;return new Promise(done=>resolve=done);});
  assert.equal(feedback.pending('history'),true);
  assert.equal(buttons[0].attributes['aria-busy'],'true');
  assert.equal(buttons[0].label.textContent,'Refresh history');
  assert.equal(buttons[0].attributes['aria-label'],'Refresh history');
  assert.equal(buttons[0].status.textContent,'Refreshing history…');
  assert.equal(await feedback.run('history',()=>{calls++;}),false);
  assert.equal(calls,1);resolve(true);await pending;
  advance(259);assert.equal(feedback.pending('history'),true);
  advance(1);assert.equal(buttons[0].dataset.refreshState,'success');
  assert.equal(buttons[0].attributes['aria-busy'],undefined);
  assert.equal(buttons[0].label.textContent,'Refresh history');
  assert.equal(buttons[0].status.textContent,'History refreshed');
  advance(1600);assert.equal(buttons[0].dataset.refreshState,'ready');assert.equal(timers.size,0);
});

test('failed reads never acknowledge success and keep their original technical error',async()=>{
  const {feedback,buttons,advance}=fixture();
  await feedback.run('inputs',async()=>false);advance(260);
  assert.equal(buttons[1].dataset.refreshState,'error');
  assert.equal(buttons[1].label.textContent,'Refresh inputs');
  assert.equal(buttons[1].status.textContent,'Input refresh failed');
  const error=new Error('EACCES: fixture device unavailable');
  await assert.rejects(feedback.run('inputs',async()=>{throw error;}),value=>value===error);
  advance(260);assert.equal(buttons[1].dataset.refreshState,'error');
  await feedback.run('inputs',async()=>true);advance(260);
  assert.equal(buttons[1].dataset.refreshState,'success');
});

test('late completions and old acknowledgement timers cannot replace newer feedback',async()=>{
  const {feedback,buttons,advance,timers}=fixture();let resolve;
  const old=feedback.run('history',()=>new Promise(done=>resolve=done));
  feedback.clear();
  await feedback.run('history',async()=>true);
  resolve(false);await old;advance(260);
  assert.equal(buttons[0].dataset.refreshState,'success');
  advance(1000);await feedback.run('history',async()=>true);
  advance(260);assert.equal(buttons[0].dataset.refreshState,'success');
  advance(340);assert.equal(buttons[0].dataset.refreshState,'success');
  feedback.clear();advance(5000);assert.equal(timers.size,0);
});
