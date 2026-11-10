import {icon} from './icons.mjs';

const labels={
  history:{ready:'Refresh history',pending:'Refreshing history…',success:'History refreshed',error:'History refresh failed'},
  inputs:{ready:'Refresh inputs',pending:'Refreshing…',success:'Inputs refreshed',error:'Input refresh failed'},
  diagnostics:{ready:'Refresh',pending:'Refreshing diagnostics…',success:'Diagnostics refreshed',error:'Diagnostics refresh failed'},
};
export function refreshButtonContent(key) {
  return `<span class="refresh-symbol" data-refresh-symbol aria-hidden="true">${icon('refresh')}</span><span class="refresh-label">${labels[key].ready}</span><span class="refresh-status" aria-live="polite" aria-atomic="true"></span>`;
}

// Feedback follows the read, not a click timer. Short reads remain perceptible;
// repeated requests and stale timer completions cannot overwrite a newer read.
export function createRefreshFeedback({changed=()=>{},now=()=>performance.now(),schedule=setTimeout,unschedule=clearTimeout}={}) {
  const states=new Map();
  function sync(root=document) {
    for(const button of root.querySelectorAll('[data-refresh-key]')){
      const key=button.dataset.refreshKey,phase=states.get(key)?.phase||'ready';
      if(!labels[key]||button.dataset.refreshState===phase)continue;
      button.dataset.refreshState=phase;
      const text=labels[key][phase];
      button.setAttribute('aria-label',labels[key].ready);
      if(phase==='pending')button.setAttribute('aria-busy','true');else button.removeAttribute('aria-busy');
      button.querySelector('[data-refresh-symbol]').innerHTML=icon(phase==='success'?'check':phase==='error'?'warning':'refresh');
      button.querySelector('.refresh-status').textContent=phase==='ready'?'':text;
    }
  }
  async function run(key,read,{settled=()=>{}}={}) {
    if(states.get(key)?.phase==='pending')return false;
    const previous=states.get(key);if(previous)unschedule(previous.timer);
    const state={phase:'pending',timer:null},started=now();states.set(key,state);changed();
    const finish=phase=>{
      if(states.get(key)!==state)return;
      const paint=()=>{
        if(states.get(key)!==state)return;
        state.phase=phase;changed();settled(phase);
        state.timer=schedule(()=>{if(states.get(key)!==state)return;states.delete(key);changed();},1600);
      };
      const remaining=Math.max(0,260-(now()-started));
      if(remaining)state.timer=schedule(paint,remaining);else paint();
    };
    try{const success=await read();finish(success===false?'error':'success');return success;}
    catch(error){finish('error');throw error;}
  }
  function clear(){for(const state of states.values())unschedule(state.timer);states.clear();}
  return {run,sync,pending:key=>states.get(key)?.phase==='pending',clear};
}
