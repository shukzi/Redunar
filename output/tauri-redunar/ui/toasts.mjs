import {toastStore} from './toast-store.mjs';
import {icon} from './icons.mjs';
const owner=Symbol.for('redunar.toast-owner');
export function toastContent(){
  return `<div class="toast-content"><p class="toast-message"></p><button type="button" class="toast-dismiss" aria-label="Dismiss notification">${icon('x')}</button></div>`;
}

export function installToasts(viewport){
  if(viewport[owner])return viewport[owner];
  const doc=viewport.ownerDocument,view=doc.defaultView,dialog=doc.querySelector('#dialog');
  const preference=view.matchMedia('(prefers-reduced-motion: reduce)');
  const controller=new AbortController(),events={signal:controller.signal};
  let card=null,currentId=null,returnFocus=null,hovered=false,focused=false,active=doc.hasFocus(),disposed=false;
  const store=toastStore({changed:render});
  function syncMotion(){
    // WebKit can retain stale CSS media results after a GTK preference change.
    viewport.dataset.reducedMotion=String(view.matchMedia(preference.media).matches);
  }
  function syncParent(){
    if(disposed)return;
    const parent=dialog?.open?dialog:doc.body;
    if(viewport.parentElement!==parent)parent.append(viewport);
  }
  function restoreFocus(){
    const target=returnFocus?.isConnected&&!returnFocus.closest('[inert]')&&(!dialog?.open||dialog.contains(returnFocus))
      ?returnFocus:dialog?.open?doc.querySelector('#dialog-title'):doc.querySelector('#workspace');
    target?.focus({preventScroll:true});
  }
  function render(){
    const entry=store.snapshot()[0];
    if(!entry){
      const restore=card?.contains(doc.activeElement);
      card?.remove();card=null;currentId=null;viewport.hidden=true;
      hovered=false;focused=false;store.hold(false);
      if(restore&&active&&!doc.hidden&&!disposed)restoreFocus();
      returnFocus=null;
      return;
    }
    syncMotion();
    syncParent();
    if(currentId!==entry.id){
      const restore=card?.contains(doc.activeElement);
      if(!card?.contains(doc.activeElement))returnFocus=doc.activeElement;
      card?.remove();
      card=doc.createElement('div');card.className='toast-card';
      card.setAttribute('aria-atomic','true');card.dataset.type=entry.type;
      card.setAttribute('role',entry.type==='error'?'alert':'status');
      card.innerHTML=toastContent();
      card.querySelector('.toast-message').textContent=entry.message;
      currentId=entry.id;
      // Capture this entry's ID so a stale click cannot dismiss its replacement.
      card.querySelector('button').addEventListener('click',()=>store.remove(entry.id));
      viewport.replaceChildren(card);
      hovered=false;focused=false;store.hold(false);
      if(restore)restoreFocus();
    }
    viewport.hidden=false;
  }
  function syncReading(){store.hold(!doc.hidden&&active&&(hovered||focused));}
  function resume(event){
    // Timers may be throttled while hidden. Recheck before painting a return.
    active=event?.type==='focus'||doc.hasFocus();hovered=false;focused=Boolean(card?.contains(doc.activeElement));
    store.hold(false);store.expire();syncReading();
  }
  viewport.addEventListener('pointerenter',event=>{if(event.pointerType==='touch')return;hovered=true;syncReading();},events);
  viewport.addEventListener('pointerleave',()=>{hovered=false;syncReading();},events);
  viewport.addEventListener('focusin',()=>{focused=true;syncReading();},events);
  viewport.addEventListener('focusout',event=>{if(viewport.contains(event.relatedTarget))return;focused=false;syncReading();},events);
  viewport.addEventListener('keydown',event=>{if(event.key==='Escape'&&card){event.preventDefault();store.remove(currentId);}},events);
  view.addEventListener('blur',()=>{active=false;hovered=false;focused=false;syncReading();},events);
  view.addEventListener('focus',resume,events);
  doc.addEventListener('visibilitychange',()=>{if(doc.hidden){hovered=false;focused=false;store.hold(false);}else resume();},events);
  doc.addEventListener('close',syncParent,{...events,capture:true});
  // Observe only the dialog. Existing notifications follow its top layer too.
  const observer=dialog?new view.MutationObserver(syncParent):null;
  if(dialog)observer.observe(dialog,{attributes:true,attributeFilter:['open']});
  view.addEventListener('pagehide',event=>{
    active=false;hovered=false;focused=false;store.hold(false);
    if(event.persisted)return;
    disposed=true;controller.abort();observer?.disconnect();store.dispose();delete viewport[owner];
  },events);
  view.addEventListener('pageshow',resume,events);
  preference.addEventListener('change',syncMotion,events);
  viewport[owner]=(message,options)=>{if(!disposed){store.expire();return store.add(message,options);}};
  return viewport[owner];
}
