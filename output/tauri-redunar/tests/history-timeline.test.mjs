import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mapSessions} from '../ui/native-data.mjs';
import {historyTimeline, historyDuration, historySelection, historySeries, historyScale, historyX, historyY, historyTime, historyKeyboardTime} from '../ui/history-timeline.mjs';

const close=(a,b)=>assert.ok(Math.abs(a-b)<1e-8,`${a} != ${b}`);
test('History preserves spikes, timestamp spacing and missing observations',()=>{
 const samples=[{elapsed:2,fps:60},{elapsed:3,fps:360},{elapsed:9,fps:null},{elapsed:10,fps:45}];
 const segments=historySeries(samples,'fps');
 assert.deepEqual(segments,[[{x:2,value:60},{x:3,value:360}],[{x:10,value:45}]]);
 const scale=historyScale(segments.flat().map(p=>p.value),'fps');
 assert.equal(scale.floor,0);assert.equal(scale.ceiling,400);
 close(historyX(3,12),225);close(historyY(360,scale),39);
 close(historyY(scale.floor,scale),210);close(historyY(scale.ceiling,scale),20);
});
test('Axes use actual extrema, explicit units and meaningful numeric tick intervals',()=>{
 assert.deepEqual(historyScale([0,37,81],'utilization').ticks,[0,25,50,75,100]);
 const fps=historyScale([30,42],'fps');assert.equal(fps.ceiling,60);assert.equal(fps.unit,'FPS');
 const frame=historyScale([.1,.35],'frame-time');assert.equal(frame.unit,'ms');assert.equal(frame.digits,1);close(frame.ceiling,.4);
 const temperature=historyScale([-12,0,61],'temperature');assert.ok(temperature.floor<=-12);assert.ok(temperature.ceiling>=61);assert.equal(temperature.unit,'°C');
 for(const metric of ['fps','frame-time','temperature','utilization']){
  const scale=historyScale([0],metric);assert.ok(scale.ceiling>scale.floor);assert.ok(scale.ticks.every(Number.isFinite));
  for(const tick of scale.ticks)assert.ok(historyY(tick,scale)>=20&&historyY(tick,scale)<=210);
 }
 assert.equal(historyScale([],'fps'),null);
});
test('Slider snaps position and inspector to the same actual observation, without extrapolating',()=>{
 const samples=[{elapsed:2,fps:50},{elapsed:4,fps:60},{elapsed:12,fps:70}];
 assert.deepEqual(historySelection(samples,8,20),{seconds:4,sample:samples[1]});
 assert.deepEqual(historySelection(samples,9,20),{seconds:12,sample:samples[2]});
 for(const value of [0,1,13,20])assert.deepEqual(historySelection(samples,value,20),{seconds:value,sample:null});
 assert.equal(historySelection(samples,-4,20).seconds,0);
 assert.equal(historySelection(samples,100,20).seconds,20);
 assert.equal(historySelection([],0,0).sample,null);
 assert.equal(historyX(0,0),0);
 assert.equal(historyDuration({duration_seconds:10},samples),12);
});
test('Keyboard visits adjacent observations at irregular and fractional times',()=>{
 const samples=[{elapsed:.1},{elapsed:.12},{elapsed:12}];
 assert.equal(historyKeyboardTime(samples,.1,'ArrowRight',20),.12);
 assert.equal(historyKeyboardTime(samples,12,'ArrowLeft',20),.12);
 assert.equal(historyKeyboardTime(samples,0,'ArrowRight',20),.1);
 assert.equal(historyKeyboardTime(samples,.12,'Home',20),0);
 assert.equal(historyKeyboardTime(samples,.12,'End',20),20);
 assert.equal(historyKeyboardTime(samples,.12,'Tab',20),null);
});
test('Invalid telemetry is unavailable in both the inspector and the chart; zero load remains valid',()=>{
 const [session]=mapSessions([{started_unix:0,timeline:[{elapsed_seconds:2,fps:-1,frame_time_ms:0,cpu_utilization_percent:101,gpu_utilization_percent:0,cpu_temperature_celsius:-4},{elapsed_seconds:1,fps:0,frame_time_ms:Infinity}]}]);
 assert.equal(session.timeline[0].elapsed,1);
 assert.equal(session.timeline[0].fps,0);
 assert.equal(session.timeline[1].fps,null);assert.equal(session.timeline[1].frameTime,null);
 assert.equal(session.timeline[1].cpuUtilization,null);assert.equal(session.timeline[1].gpuUtilization,0);
 assert.equal(session.timeline[1].cpuTemperature,-4);
 assert.deepEqual(historySeries(session.timeline,'fps'),[[{x:1,value:0}]]);
 // Cursor updates reuse the bounded timeline, rather than rebuilding it per input.
 assert.equal(historyTimeline(session).samples,session.timeline);
});
test('Legacy FPS is the uncapped reciprocal of retained intervals, only within the retained tail',()=>{
 const {samples,legacy}=historyTimeline({duration_seconds:10,frames:[.5,20]});
 assert.equal(legacy,true);assert.equal(samples[0].fps,2000);assert.equal(samples[1].fps,50);
 close(samples[0].elapsed,9.98);close(samples[1].elapsed,10);
 assert.equal(historySelection(samples,5,10).sample,null);
 assert.equal(historyTime(samples[0].elapsed),'00:09.98');
 assert.equal(historyTime(59.9999),'01:00');assert.equal(historyTime(3600),'60:00');
 const short=historyTimeline({duration_seconds:0,frames:[10,20]}).samples;
 close(short[0].elapsed,.01);close(short[1].elapsed,.03);
});

test('Load scales to measured peaks within percentage bounds and keeps fractional observations',()=>{
 for(const peak of [0,.15,37.4256,60,99,100]){
  const scale=historyScale([0,peak],'utilization');
  assert.equal(scale.floor,0);assert.ok(scale.ceiling>=peak&&scale.ceiling<=100);
  close(210-historyY(peak,scale),peak/scale.ceiling*190);
 }
 assert.deepEqual(historyScale([0,37.4256],'utilization').ticks,[0,10,20,30,40]);
 const samples=[{elapsed:0,cpuUtilization:24.9999},{elapsed:3,cpuUtilization:25.0001},{elapsed:12,cpuUtilization:37.4256}];
 const scale=historyScale(samples.map(s=>s.cpuUtilization),'utilization');
 assert.ok(historyY(samples[0].cpuUtilization,scale)>historyY(25,scale));
 assert.ok(historyY(samples[1].cpuUtilization,scale)<historyY(25,scale));
 assert.equal(historySelection(samples,3,12).sample.cpuUtilization,25.0001);
 assert.equal(historySeries(samples,'cpuUtilization')[0][2].value,37.4256);
});
