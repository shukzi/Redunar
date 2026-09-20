import { validHistoryValue } from './history-timeline.mjs';

// Runtime data crosses one explicit boundary. Never reuse the sandbox fixtures
// or a cached observation after a failed native request.
export const number = value => typeof value === 'number' && Number.isFinite(value) ? value : null;
export const measurement = (value, digits=0) => number(value) === null ? '—' : value.toFixed(digits);
export const duration = value => number(value) === null ? '—' : `${Math.floor(value/60)}m ${Math.floor(value%60)}s`;
export function visibleSession(status) {
  return status && status.phase === 'Ended' && !status.can_end && !status.launch_locked ? null : status ?? null;
}

const profileDraftKeys = ['overlay','preset','layout','palette','position','scale','opacity','metrics','captureMetrics','replay','fps','quality','format'];
const normalizedMetrics = value => [...(Array.isArray(value)?value:[])].sort();
export function profileDraftChanged(current,saved) {
  return profileDraftKeys.some(key=>key==='metrics'
    ? JSON.stringify(normalizedMetrics(current?.[key]))!==JSON.stringify(normalizedMetrics(saved?.[key]))
    : current?.[key]!==saved?.[key]);
}
export function shortcutDraftChanged(current,saved) {
  return JSON.stringify(Array.isArray(current)?current:[])!==JSON.stringify(Array.isArray(saved)?saved:[]);
}
export function overrideDraftChanged(current,saved) {
  return ['overlay','captureMetrics'].some(key=>
    (Object.hasOwn(current||{},key)?current[key]:null)!==(Object.hasOwn(saved||{},key)?saved[key]:null));
}

export function mapGames(records) {
  if (!Array.isArray(records)) throw new Error('Invalid catalog response');
  return records.map(g=>({...g,id:String(g.id),exe:g.executable,args:Array.isArray(g.arguments)?g.arguments:[],workdir:g.working_directory||'',mark:g.name.slice(0,2).toUpperCase(),color:'sand',launcher:g.steam_app_id?'Steam':'Local game',overrides:{...g.overrides}}));
}
export function mapClips(records) {
  if (!Array.isArray(records)) throw new Error('Invalid clip response');
  return records.map(c=>({...c,id:c.file_name,game:c.game_name||null,duration:Number.isFinite(c.duration_seconds)&&c.duration_seconds>0?c.duration_seconds:null,size:`${(c.bytes/1048576).toFixed(1)} MB`,theme:'sand',date:new Date(Number(BigInt(c.modified_unix_ns)/1000000n)).toLocaleString([], {month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'}),thumbnail:null}));
}
const historyValue=(key,value)=>validHistoryValue(key,value)?value:null;
export function mapSessions(records) {
  if (!Array.isArray(records)) throw new Error('Invalid history response');
  return records.slice().sort((a,b)=>(Number(b.started_unix)||0)-(Number(a.started_unix)||0)||(Number(b.id)||0)-(Number(a.id)||0)).map(s=>({...s,date:new Date(s.started_unix*1000).toLocaleDateString(undefined,{month:'long',day:'numeric',year:'numeric'}),time:new Date(s.started_unix*1000).toLocaleTimeString(undefined,{hour:'2-digit',minute:'2-digit'}),duration:duration(s.duration_seconds),averageFps:number(s.average_fps),onePercentLowFps:number(s.one_percent_low_fps),pointOnePercentLowFps:number(s.point_one_percent_low_fps),fps:measurement(s.average_fps),low:measurement(s.one_percent_low_fps),lowest:measurement(s.point_one_percent_low_fps),frames:(s.frame_intervals_ns||[]).filter(n=>number(n)!==null&&n>0).slice(-240).map(n=>n/1e6),timeline:(Array.isArray(s.timeline)?s.timeline:[]).map(sample=>({elapsed:number(sample.elapsed_seconds),fps:historyValue('fps',sample.fps),frameTime:historyValue('frameTime',sample.frame_time_ms),cpuTemperature:historyValue('cpuTemperature',sample.cpu_temperature_celsius),gpuTemperature:historyValue('gpuTemperature',sample.gpu_temperature_celsius),cpuUtilization:historyValue('cpuUtilization',sample.cpu_utilization_percent),gpuUtilization:historyValue('gpuUtilization',sample.gpu_utilization_percent)})).filter(sample=>sample.elapsed!==null&&sample.elapsed>=0).sort((a,b)=>a.elapsed-b.elapsed)}));
}
export function mapDefaults(workspace) {
  const s=workspace.values;
  return {overlay:s.overlay,preset:s.preset,layout:s.layout,palette:s.palette,position:s.position,scale:s.scale,opacity:s.opacity,metrics:[...s.metrics],captureMetrics:s.captureMetrics,replay:s.replayEnabled,fps:s.fps,quality:s.quality,format:s.format};
}
export function profilePayload(draft, shortcuts) {
  return {...draft,replayEnabled:draft.replay,storageLimit:'Unlimited',shortcuts};
}
export function overridePayload(overrides) {
  return Object.fromEntries(['overlay','captureMetrics'].map(key=>[key,Object.hasOwn(overrides,key)?overrides[key]:null]));
}
