import {toastStore} from './toast-store.mjs';
import {springMotion} from './spring-motion.mjs';
import {icon} from './icons.mjs';
const owner=Symbol.for('redunar.toast-owner');
export function toastContent(type='info'){
  return `<div class="toast-content"><span class="toast-icon">${icon(type==='success'?'success':type==='error'||type==='warning'?'warning':'info')}</span><p class="toast-message"></p><button type="button" class="toast-dismiss" aria-label="Dismiss notification">${icon('x')}</button></div><span class="toast-progress" aria-hidden="true"></span>`;
}

export function installToasts(viewport){
  if(viewport[owner])return viewport[owner];
  const doc=viewport.ownerDocument,view=doc.defaultView,preference=view.matchMedia('(prefers-reduced-motion: reduce)');
  const mounted=new Map(),exiting=new Map(),controller=new AbortController(),events={signal:controller.signal};
  let hovered=false,focused=false,expanded=false,disposed=false;
  const transform=value=>`translate3d(${value.x}px,${value.y}px,0) scale(${value.scale})`;
  const store=toastStore({changed:render});
  function removeElement(id,state){
    clearTimeout(state.delay);state.motion.stop();state.progress?.cancel();state.exit?.cancel();state.card.remove();exiting.delete(id);
    if(!mounted.size&&!exiting.size){
      viewport.hidden=true;hovered=false;focused=false;expanded=false;viewport.dataset.expanded='false';
      store.resume('hover');store.resume('focus');
    }
  }
  function dismiss(id,state){
    clearTimeout(state.delay);state.progress?.cancel();
    const value=state.motion.snapshot().value;state.motion.stop();
    state.card.inert=true;state.card.setAttribute('aria-hidden','true');exiting.set(id,state);
    if(preference.matches){removeElement(id,state);return;}
    // A departing backing card must not travel below a remaining toast and
    // expose its empty surface. Keep it behind the live stack while it fades.
    const stacked=store.snapshot().length>0;
    if(stacked)state.card.style.zIndex='0';
    state.exit=state.card.animate([
      {transform:transform(value),opacity:value.opacity,filter:`blur(${Math.max(0,value.blur)}px)`},
      {transform:transform(stacked?value:{...value,y:value.y+value.h+24,scale:.96}),opacity:0,filter:stacked?'none':'blur(4px)'},
    ],{duration:200,easing:'cubic-bezier(.33,.17,.67,.5)',fill:'both'});
    state.exit.onfinish=()=>{if(exiting.get(id)===state)removeElement(id,state);};
    // Bursts remain bounded even when several notifications replace old ones.
    while(exiting.size>3){const [oldId,old]=exiting.entries().next().value;removeElement(oldId,old);}
  }
  function create(entry){
    const card=doc.createElement('div');card.className='toast-card';card.dataset.type=entry.type;
    card.setAttribute('role',entry.type==='error'?'alert':'status');card.setAttribute('aria-atomic','true');
    card.innerHTML=toastContent(entry.type);
    card.querySelector('.toast-message').textContent=entry.message;
    const returnFocus=doc.activeElement;
    card.querySelector('button').addEventListener('click',()=>{
      const restore=card.contains(doc.activeElement);store.remove(entry.id);
      if(restore)(mounted.get(store.snapshot().at(-1)?.id)?.card.querySelector('button')||(returnFocus?.isConnected?returnFocus:null))?.focus({preventScroll:true});
    });
    viewport.append(card);
    const content=card.querySelector('.toast-content'),height=content.offsetHeight+2;
    const state={card,content,height,delay:null,progress:null,exit:null,motion:null};
    state.motion=springMotion({x:0,y:height+24,scale:.96,opacity:0,blur:4,h:height},value=>{
      card.style.transform=transform(value);card.style.opacity=String(Math.max(0,Math.min(1,value.opacity)));
      card.style.filter=value.blur>.02?`blur(${Math.max(0,value.blur)}px)`:'none';card.style.height=`${Math.max(0,value.h)}px`;
    },{stiffness:280,damping:24,maxDuration:900});
    return state;
  }
  function layout(cascade=false){
    const entries=store.snapshot().reverse(),front=mounted.get(entries[0]?.id);if(!front)return;
    let offset=0;
    entries.forEach((entry,index)=>{
      const state=mounted.get(entry.id);clearTimeout(state.delay);
      state.height=state.content.offsetHeight+2;
      state.card.style.zIndex=String(10-index);state.card.dataset.behind=String(index>0&&!expanded);
      state.card.inert=index>0&&!expanded;
      const target={x:0,y:expanded?-offset:-index*12,scale:expanded?1:1-index*.05,opacity:1,blur:0,h:expanded?state.height:front.height};
      const move=()=>{state.delay=null;state.motion.to(target,{immediate:preference.matches});};
      if(cascade&&index&&!preference.matches)state.delay=setTimeout(move,index*30);else move();
      offset+=state.height+12;
    });
    viewport.style.height=`${expanded?offset-12:front.height+(entries.length-1)*12}px`;
  }
  function render(){
    const entries=store.snapshot(),ids=new Set(entries.map(entry=>entry.id));
    let focusEvicted=false;
    for(const [id,state] of mounted)if(!ids.has(id)){focusEvicted ||= state.card.contains(doc.activeElement);mounted.delete(id);dismiss(id,state);}
    if(!entries.length)return;
    const parent=doc.querySelector('#dialog[open]')||doc.body;if(viewport.parentElement!==parent)parent.append(viewport);
    viewport.hidden=false;
    for(const entry of entries){
      let state=mounted.get(entry.id);if(!state){state=create(entry);mounted.set(entry.id,state);}
      state.progress?.cancel();const bar=state.card.querySelector('.toast-progress'),fraction=entry.remaining/entry.duration;
      bar.style.transform=`scaleX(${fraction})`;
      if(!entry.paused)state.progress=bar.animate([{transform:`scaleX(${fraction})`},{transform:'scaleX(0)'}],{duration:entry.remaining,easing:'linear',fill:'forwards'});
    }
    layout();
    if(focusEvicted)mounted.get(entries.at(-1)?.id)?.card.querySelector('button').focus({preventScroll:true});
  }
  function expand(){const next=hovered||focused;if(next!==expanded){expanded=next;viewport.dataset.expanded=String(expanded);layout(true);}}
  viewport.addEventListener('pointerenter',event=>{if(event.pointerType==='touch')return;hovered=true;store.pause('hover');expand();},events);
  viewport.addEventListener('pointerleave',()=>{hovered=false;store.resume('hover');expand();},events);
  viewport.addEventListener('focusin',()=>{focused=true;store.pause('focus');expand();},events);
  viewport.addEventListener('focusout',event=>{if(viewport.contains(event.relatedTarget))return;focused=false;store.resume('focus');expand();},events);
  viewport.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();(event.target.closest('.toast-card')||mounted.get(store.snapshot().at(-1)?.id)?.card)?.querySelector('button').click();}},events);
  doc.addEventListener('visibilitychange',()=>doc.hidden?store.pause('hidden'):store.resume('hidden'),events);
  view.addEventListener('blur',()=>store.pause('window'),events);
  view.addEventListener('focus',()=>store.resume('window'),events);
  view.addEventListener('resize',()=>layout(),events);
  doc.addEventListener('close',event=>{
    const dialog=viewport.closest('dialog');
    if(dialog===event.target&&!dialog.open){doc.body.append(viewport);layout();}
  },{...events,capture:true});
  preference.addEventListener('change',()=>{
    if(preference.matches){for(const [id,state] of exiting)removeElement(id,state);for(const state of mounted.values())state.motion.finish();}
    layout();
  },events);
  view.addEventListener('pagehide',event=>{
    if(event.persisted){store.pause('page');return;}
    disposed=true;controller.abort();store.dispose();delete viewport[owner];
    for(const [id,state] of exiting)removeElement(id,state);
  },events);
  view.addEventListener('pageshow',()=>store.resume('page'),events);
  if(doc.hidden)store.pause('hidden');
  if(!doc.hasFocus())store.pause('window');
  viewport[owner]=(message,options)=>{if(!disposed&&String(message).trim())return store.add(message,options);};
  return viewport[owner];
}
