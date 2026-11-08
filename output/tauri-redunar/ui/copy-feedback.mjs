import {icon} from './icons.mjs';

export function copyButtonContent(copied=false) {
  return `<span class="copy-button-symbol" data-copied="${copied}">${icon('copy','copy-glyph-original')}${icon('check','copy-glyph-confirmed')}</span><span class="copy-button-label">Copy</span><span class="copy-button-status" aria-live="polite" aria-atomic="true">${copied?'Copied':''}</span>`;
}
function paint(button,copied) {
  button.dataset.copyState=copied?'copied':'ready';
  const status=button.querySelector('.copy-button-status'),symbol=button.querySelector('.copy-button-symbol');
  if(status)status.textContent=copied?'Copied':'';
  if(symbol)symbol.dataset.copied=String(copied);
}
// A late clipboard response can update only its original mounted button. A
// second click is ignored while writing; subsequent successes restart the timer.
export function createCopyFeedback({render=paint,setTimer=setTimeout,clearTimer=clearTimeout}={}) {
  const states=new WeakMap();
  return async function copy(button,write) {
    const state=states.get(button)||{pending:false,timer:null};
    if(state.pending||button.disabled)return false;
    states.set(button,state);state.pending=true;
    const doc=button.ownerDocument,restoreFocus=doc?.activeElement===button;
    button.disabled=true;button.setAttribute('aria-busy','true');
    try {
      await write();
      if(state.timer!==null)clearTimer(state.timer);
      state.timer=null;
      if(button.isConnected){
        render(button,true);
        state.timer=setTimer(()=>{state.timer=null;if(button.isConnected)render(button,false);},3000);
      }
      return true;
    } catch(error) {
      if(state.timer!==null)clearTimer(state.timer);
      state.timer=null;
      if(button.isConnected)render(button,false);
      throw error;
    } finally {
      state.pending=false;button.disabled=false;button.removeAttribute('aria-busy');
      if(restoreFocus&&button.isConnected&&(doc.activeElement===button||doc.activeElement===doc.body))button.focus({preventScroll:true});
    }
  };
}
export const copyFeedback=createCopyFeedback();
