import {test} from 'node:test';
import assert from 'node:assert/strict';
import {captureHistoryGraph,historyGraphMotion} from '../ui/history-motion.mjs';

function graph(values,{kind='primary',xs=values.map((_,i)=>i),circle=false}={}) {
  const attrs={points:values.map((y,i)=>`${xs[i]},${y}`).join(' ')};
  const areaAttrs={points:`0,210 ${attrs.points} ${xs.at(-1)},210`};
  const area={matches:()=>true,getAttribute:key=>areaAttrs[key],setAttribute:(key,value)=>areaAttrs[key]=value,removeAttribute:key=>delete areaAttrs[key]};
  const line={classList:{contains:value=>value===kind},previousElementSibling:area,
    getAttribute:key=>attrs[key],setAttribute:(key,value)=>attrs[key]=value,removeAttribute:key=>delete attrs[key]};
  // SVGPointList has a separate count, unlike a normal array.
  Object.defineProperty(line,'points',{get(){const points=attrs.points.split(' ').map(pair=>{const [x,y]=pair.split(',').map(Number);return {x,y};});points.numberOfItems=points.length;return points;}});
  return {dataset:{},isConnected:true,querySelector:()=>circle?{}:null,querySelectorAll:()=>[line],line,area,attrs,areaAttrs};
}
function fixture(){
  let time=0,id=0;const queue=new Map();
  const motion=historyGraphMotion({now:()=>time,requestFrame:fn=>{queue.set(++id,fn);return id;},cancelFrame:id=>queue.delete(id)});
  return {motion,queue,step:delta=>{time+=delta;const work=[...queue.values()];queue.clear();work.forEach(fn=>fn(time));}};
}
test('morph reaches exact destination and area, then has no idle work',()=>{
  const f=fixture(),a=graph([10,90,20]),b=graph([90,10,80]);
  const exact=b.attrs.points,area=b.areaAttrs.points;
  f.motion.start(captureHistoryGraph(a),b);assert.equal(b.attrs.points,a.attrs.points);
  f.step(260);assert.notEqual(b.attrs.points,a.attrs.points);assert.notEqual(b.attrs.points,exact);
  f.step(260);assert.equal(b.attrs.points,exact);assert.equal(b.areaAttrs.points,area);
  assert.equal(f.queue.size,0);assert.equal(b.dataset.morphing,undefined);
});
test('rapid switching starts at current geometry and cancels the previous frame',()=>{
  const f=fixture(),a=graph([10,20]),b=graph([90,80]),c=graph([40,60]);
  f.motion.start(captureHistoryGraph(a),b);f.step(150);
  const current=b.attrs.points,from=captureHistoryGraph(b),exact=c.attrs.points;
  f.motion.start(from,c);assert.equal(c.attrs.points,current);assert.equal(f.queue.size,1);
  f.step(520);assert.equal(c.attrs.points,exact);assert.equal(f.queue.size,0);
});
test('settling and disconnected cleanup cancel work and restore exact geometry',()=>{
  for(const disconnect of [false,true]){
    const f=fixture(),b=graph([90,80]),exact=b.attrs.points;
    f.motion.start(captureHistoryGraph(graph([10,20])),b);f.step(100);
    if(disconnect){b.isConnected=false;f.step(10);}else f.motion.settle();
    assert.equal(b.attrs.points,exact);assert.equal(f.queue.size,0);assert.equal(b.dataset.morphing,undefined);
  }
});
test('different coverage fades exact segments without inventing source observations',()=>{
  const f=fixture(),a=captureHistoryGraph(graph([10,20]));
  const b=graph([50,60],{xs:[0,2]}),exact=b.attrs.points;
  f.motion.start(a,b);
  assert.equal(b.attrs.points,exact);assert.equal(b.attrs.opacity,0);
  f.step(200);assert.equal(b.attrs.points,exact);assert.ok(b.attrs.opacity>0&&b.attrs.opacity<1);
  f.step(520);assert.equal(b.attrs.points,exact);assert.equal(b.attrs.opacity,undefined);assert.equal(f.queue.size,0);
  assert.equal(captureHistoryGraph(null),null);
});
test('entering a sensor with later coverage morphs from matching recorded timestamps',()=>{
  const f=fixture(),a=graph([10,20,30,40]),b=graph([90,80],{xs:[2,3]}),exact=b.attrs.points;
  f.motion.start(captureHistoryGraph(a),b);
  assert.equal(b.attrs.points,'2,30 3,40');
  f.step(200);assert.notEqual(b.attrs.points,exact);assert.notEqual(b.attrs.points,'2,30 3,40');
  f.step(520);assert.equal(b.attrs.points,exact);assert.equal(f.queue.size,0);
});

test('dense transitions preserve extrema with bounded work and restore every exact point',()=>{
  const f=fixture(),values=Array(7200).fill(60);values[3721]=5;values[6000]=170;
  const a=graph(Array(7200).fill(120)),b=graph(values),exact=b.attrs.points;
  f.motion.start(captureHistoryGraph(a),b);f.step(260);
  assert.ok(b.line.points.length<=1024);assert.equal(captureHistoryGraph(b)[0].points.length,7200);
  assert.ok(b.line.points.some(p=>p.x===3721));assert.ok(b.line.points.some(p=>p.x===6000));
  f.step(260);assert.equal(b.attrs.points,exact);assert.equal(b.line.points.length,7200);
  assert.equal(b.line.historyMotionPoints,undefined);assert.equal(f.queue.size,0);
});
test('rapid dense switches retain full current shape and exact final data',()=>{
  const f=fixture(),a=graph(Array(7200).fill(10)),b=graph(Array(7200).fill(90)),c=graph(Array(7200).fill(40));
  f.motion.start(captureHistoryGraph(a),b);f.step(150);
  const from=captureHistoryGraph(b),current=from[0].points[3000].y,exact=c.attrs.points;
  f.motion.start(from,c);assert.ok(Math.abs(captureHistoryGraph(c)[0].points[3000].y-current)<1e-9);
  assert.equal(f.queue.size,1);f.step(520);assert.equal(c.attrs.points,exact);assert.equal(f.queue.size,0);
});
