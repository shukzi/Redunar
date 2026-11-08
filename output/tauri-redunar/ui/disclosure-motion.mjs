// Shared desktop motion for native details, including dynamically rendered panels.
import {setPopupExpanded,cancelPopupMotion} from './popup-motion.mjs';
const active=new WeakMap();
const installed=new WeakSet();
const timing={duration:220,easing:'cubic-bezier(.2,0,0,1)'};

function cancel(details,state) {
 if(!state)return;
 active.delete(details);
 if(state.animation){state.animation.onfinish=null;state.animation.cancel();}
 if(state.floating)cancelPopupMotion(state.content);
 for(const [name,value] of Object.entries(state.styles))details.style[name]=value;
 state.content.inert=state.inert;
 delete details.dataset.expanded;
}

export function setDisclosureOpen(details,expanded,{immediate=false}={}) {
 if(!details)return;
 const summary=details.querySelector(':scope > summary'),content=summary?.nextElementSibling;
 if(!summary||!content){details.open=expanded;return;}
 const previous=active.get(details),view=details.ownerDocument.defaultView;
 if(previous?.target===expanded&&!immediate)return;
 if(!previous&&details.open===expanded)return;
 const start=details.getBoundingClientRect().height;
 const contentStyle=view.getComputedStyle(content);
 const floating=['absolute','fixed'].includes(contentStyle.position);
 // Read the painted frame before cancelling so rapid toggles reverse smoothly.
 // Floating surfaces retain their own animation until the new target samples it.
 if(floating&&previous)previous.floating=false;
 cancel(details,previous);
 if(immediate||view.matchMedia('(prefers-reduced-motion: reduce)').matches||!details.animate){cancelPopupMotion(content);details.open=expanded;return;}
 const state={target:expanded,content,inert:content.inert,styles:{height:details.style.height,overflow:details.style.overflow,boxSizing:details.style.boxSizing},animation:null};
 active.set(details,state);
 details.dataset.expanded=String(expanded);
 details.open=true;
 content.inert=!expanded;
 const finish=()=>{if(active.get(details)!==state)return;cancel(details,state);details.open=expanded;};
 let frames;
 if(floating){
  state.floating=true;
  setPopupExpanded(content,expanded,{onfinish:finish});
  return;
 }else{
  details.style.boxSizing='border-box';details.style.height='auto';
  const style=view.getComputedStyle(details);
  const end=expanded?details.getBoundingClientRect().height:summary.getBoundingClientRect().height+parseFloat(style.paddingTop)+parseFloat(style.paddingBottom)+parseFloat(style.borderTopWidth)+parseFloat(style.borderBottomWidth);
  details.style.height=`${start}px`;details.style.overflow='hidden';
  frames=[{height:`${start}px`},{height:`${end}px`}];
 }
 state.animation=(floating?content:details).animate(frames,timing);
 state.animation.onfinish=finish;
}

export function installDisclosureMotion(root=document) {
 if(installed.has(root))return;
 installed.add(root);
 root.addEventListener('click',event=>{
  const summary=event.target.closest('summary');
  if(!summary||summary.parentElement?.tagName!=='DETAILS'||event.target.closest('a,button,input,select,textarea'))return;
  event.preventDefault();
  const details=summary.parentElement;
  setDisclosureOpen(details,!(active.get(details)?.target??details.open));
 },true);
 // External dismissal (menu actions, rerenders) must not be undone by a late finish.
 root.addEventListener('toggle',event=>{
  if(!event.target.open)cancel(event.target,active.get(event.target));
 },true);
 root.addEventListener('invalid',event=>{
  let details=event.target.closest('details');
  while(details){setDisclosureOpen(details,true,{immediate:true});details=details.parentElement?.closest('details');}
 },true);
}
