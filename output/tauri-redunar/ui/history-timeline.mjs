// History uses the journal's bounded observations, without averaging their
// timestamps or amplitudes. Never extend a measurement into unrecorded time.
export const historyMetrics = {
  fps: {unit:'FPS', series:[['fps','FPS','primary']]},
  'frame-time': {unit:'ms', series:[['frameTime','Frame time','primary']]},
  temperature: {unit:'°C', series:[['cpuTemperature','CPU','cpu'],['gpuTemperature','GPU','gpu']]},
  utilization: {unit:'%', series:[['cpuUtilization','CPU','cpu'],['gpuUtilization','GPU','gpu']]},
};

export function validHistoryValue(key, value) {
  if(!Number.isFinite(value))return false;
  if(key.endsWith('Temperature'))return true;
  if(key.endsWith('Utilization'))return value>=0&&value<=100;
  return key==='frameTime'?value>0:value>=0;
}

export function historyTimeline(session) {
  if(session.timeline?.length)return {samples:session.timeline,legacy:false};
  const frames=(session.frames||[]).filter(value=>Number.isFinite(value)&&value>0);
  const duration=Number.isFinite(session.duration_seconds)?Math.max(0,session.duration_seconds):0;
  const retainedSeconds=frames.reduce((sum,value)=>sum+value,0)/1000;
  const end=duration||retainedSeconds;
  let remaining=retainedSeconds;
  return {samples:frames.map((frameTime,index)=>{
    remaining-=frameTime/1000;
    const elapsed=index===frames.length-1?end:end-remaining;
    return {elapsed,fps:1000/frameTime,frameTime,cpuTemperature:null,gpuTemperature:null,cpuUtilization:null,gpuUtilization:null};
  }).filter(sample=>sample.elapsed>=0),legacy:true};
}

export function historyDuration(session, samples) {
  return Math.max(0,Number.isFinite(session.duration_seconds)?session.duration_seconds:0,samples.at(-1)?.elapsed||0);
}

export function closestHistorySample(samples, seconds) {
  if(!samples.length)return null;
  // A native range may round its serialized fractional value. Preserve an exact
  // endpoint selection across that round trip, without filling unrecorded time.
  if(Math.abs(seconds-samples[0].elapsed)<1e-9)return samples[0];
  if(Math.abs(seconds-samples.at(-1).elapsed)<1e-9)return samples.at(-1);
  if(seconds<samples[0].elapsed||seconds>samples.at(-1).elapsed)return null;
  let low=0,high=samples.length-1;
  while(low<high){const middle=Math.floor((low+high)/2);if(samples[middle].elapsed<seconds)low=middle+1;else high=middle;}
  const after=samples[low],before=samples[Math.max(0,low-1)];
  return seconds-before.elapsed<=after.elapsed-seconds?before:after;
}

export function historySelection(samples, value, duration) {
  const requested=Math.min(duration,Math.max(0,Number(value)||0));
  const sample=closestHistorySample(samples,requested);
  return {seconds:sample?.elapsed??requested,sample};
}

export function historySeries(samples, key) {
  const segments=[];
  let segment=[];
  for(const sample of samples){
    if(validHistoryValue(key,sample[key]))segment.push({x:sample.elapsed,value:sample[key]});
    else if(segment.length){segments.push(segment);segment=[];}
  }
  if(segment.length)segments.push(segment);
  return segments;
}

export function historyScale(values, metric) {
  const finite=values.filter(Number.isFinite);
  if(!finite.length)return null;
  const min=Math.min(...finite),max=Math.max(...finite);
  const lower=metric==='temperature'?min-5:0;
  const upper=metric==='temperature'?max+5:Math.max(max,metric==='utilization'?1:0.001);
  const rough=(upper-lower)/4;
  const magnitude=10**Math.floor(Math.log10(rough));
  let step=[1,2,2.5,5,10].map(n=>n*magnitude).find(n=>n>=rough);
  if(metric==='fps')step=Math.max(1,step);
  const floor=metric==='utilization'?0:Math.floor(lower/step)*step;
  const ceiling=metric==='utilization'?Math.min(100,Math.ceil(upper/step)*step):Math.ceil(upper/step)*step;
  const digits=Math.max(0,Math.min(6,-Math.floor(Math.log10(step))+(Number.isInteger(step/10**Math.floor(Math.log10(step)))?0:1)));
  const ticks=Array.from({length:Math.round((ceiling-floor)/step)+1},(_,i)=>Number((floor+i*step).toFixed(8)));
  return {floor,ceiling,ticks,digits,unit:(historyMetrics[metric]||historyMetrics.fps).unit};
}

export const historyX = (seconds,duration) => duration>0?seconds/duration*900:0;
export const historyY = (value,scale) => 210-(value-scale.floor)/(scale.ceiling-scale.floor)*190;
export function historyTime(seconds) {
  if(!Number.isFinite(seconds))return '—';
  const millis=Math.round(Math.max(0,seconds)*1000),whole=Math.floor(millis/1000);
  const base=`${String(Math.floor(whole/60)).padStart(2,'0')}:${String(whole%60).padStart(2,'0')}`;
  return millis%1000?`${base}.${String(millis%1000).padStart(3,'0').replace(/0+$/,'')}`:base;
}

export function historyKeyboardTime(samples, current, key, duration) {
  if(key==='Home')return 0;
  if(key==='End')return duration;
  if(key==='ArrowRight'||key==='ArrowUp')return samples.find(sample=>sample.elapsed>current)?.elapsed??duration;
  if(key==='ArrowLeft'||key==='ArrowDown')return samples.findLast(sample=>sample.elapsed<current)?.elapsed??0;
  if(key==='PageUp'||key==='PageDown')return Math.min(duration,Math.max(0,current+(key==='PageUp'?1:-1)*duration/10));
  return null;
}
