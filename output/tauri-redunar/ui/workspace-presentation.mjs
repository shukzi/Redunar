// Presentation only. Native settings, drafts and graph data keep their owners.
import {springMotion} from './spring-motion.mjs';
const root=document.documentElement;
const reducedPreference=matchMedia('(prefers-reduced-motion: reduce)');
const indicators=new Map(),observed=new Map();
const paintedPlates=new WeakMap();
let pending=false,snapNext=false;
function schedule(){if(pending)return;pending=true;requestAnimationFrame(()=>{pending=false;refreshWorkspacePresentation();});}
function setPlateGeometry(plate,geometry){
 const styles={width:`${Math.max(0,geometry.w)}px`,height:`${Math.max(0,geometry.h)}px`,transform:`translate(${geometry.x}px,${geometry.y}px)`,borderRadius:`${geometry.radius}px`};
 const previous=paintedPlates.get(plate)||{};
 for(const [name,value] of Object.entries(styles))if(previous[name]!==value)plate.style[name]=value;
 paintedPlates.set(plate,styles);
}
function measureIndicator(group,kind,active,key){
 if(!active)return;
 const box=active.getBoundingClientRect(),parent=group.getBoundingClientRect();if(!box.width||!box.height)return;
 const geometry=kind==='nav'?{x:0,y:box.top-parent.top+group.scrollTop+11,w:3,h:box.height-22,radius:2}:
  {x:box.left-parent.left-group.clientLeft+group.scrollLeft,y:box.top-parent.top-group.clientTop+group.scrollTop,w:box.width,h:box.height,radius:Number.parseFloat(getComputedStyle(active).borderRadius)||0};
 const scope=kind==='nav'?'shell':group.closest('#dialog')?'dialog':root.dataset.page;
 return {group,kind,id:scope+':'+key,geometry};
}
function indicator({group,kind,id,geometry},seen){
 seen.add(id);
 let state=indicators.get(id),plate=state?.plate||group.querySelector(':scope>.selection-indicator');
 if(!plate){
  plate=document.createElement('i');plate.className=`selection-indicator selection-indicator-${kind}`;plate.setAttribute('aria-hidden','true');
  if(kind==='segment'||kind==='tabs'){plate.style.background='#242424';plate.style.borderColor='#444';}
 }
 if(plate.parentElement!==group)group.append(plate);
 if(group.dataset.selectionIndicator!==kind)group.dataset.selectionIndicator=kind;
 const paint=geometry=>{if(!plate.isConnected){state?.motion.stop();return;}setPlateGeometry(plate,geometry);};
 if(!state){
  state={plate,group,motion:springMotion(geometry,paint,{onRest:()=>plate.removeAttribute('data-moving')})};indicators.set(id,state);
 } else {state.group=group;state.motion.bind(paint);state.motion.to(geometry,{immediate:reducedPreference.matches||snapNext});}
 if(state.motion.running&&!plate.hasAttribute('data-moving'))plate.dataset.moving='true';
 if(!observed.has(group)){
  const observer=new ResizeObserver(schedule);observer.observe(group);
  for(const button of group.querySelectorAll(':scope>button'))observer.observe(button);
  observed.set(group,observer);
 }
}
export function refreshWorkspacePresentation(){
 for(const [group,observer] of observed)if(!group.isConnected){observer.disconnect();observed.delete(group);}
 // Read every group before changing any plate. Resize and retained-page
 // refreshes must not alternate layout reads with width/height writes.
 const seen=new Set(),measurements=[],nav=document.querySelector('#navigation');
 const measure=(...args)=>{const value=measureIndicator(...args);if(value)measurements.push(value);};
 if(nav)measure(nav,'nav',nav.querySelector('.nav-link.active'),'navigation');
 document.querySelectorAll('.tabbar,.history-view-toggle,.overview-view-toggle').forEach(group=>{
  const buttons=[...group.querySelectorAll(':scope>button')];
  measure(group,'tabs',buttons.find(button=>button.classList.contains('active')),'tabs:'+buttons.map(button=>button.dataset.globalTab||button.dataset.libraryTab||button.dataset.overlayEditorTab||button.dataset.historyView||button.dataset.overviewView||button.textContent.trim()).join('|'));
 });
 const clips=document.querySelector('.clip-collection');if(clips)measure(clips,'segment',clips.querySelector('[aria-pressed=true]'),'clip-collection');
 document.querySelectorAll('.choice-switcher').forEach(group=>measure(group,'segment',group.querySelector('button[aria-pressed=true]'),'choice:'+(group.dataset.choiceGroup||group.getAttribute('aria-label'))));
 for(const measurement of measurements)indicator(measurement,seen);
 for(const [id,state] of indicators)if(!seen.has(id)){state.motion.stop();state.plate.remove();indicators.delete(id);}
 snapNext=false;
}
const retainedSelector='.tabbar,.history-view-toggle,.overview-view-toggle,.clip-collection';
const controlKey=group=>[...group.querySelectorAll(':scope>button')].map(button=>button.dataset.globalTab||button.dataset.libraryTab||button.dataset.overlayEditorTab||button.dataset.historyView||button.dataset.overviewView||button.dataset.clipScope||button.textContent.trim()).join('|');
export function retainControlGroups(workspace,enabled){
 const saved=new Map(enabled?[...workspace.querySelectorAll(retainedSelector)].map(group=>[controlKey(group),group]):[]),focused=document.activeElement;
 return ()=>{
  for(const replacement of workspace.querySelectorAll(retainedSelector)){
   const group=saved.get(controlKey(replacement));if(!group)continue;
   const buttons=[...group.querySelectorAll(':scope>button')],next=[...replacement.querySelectorAll(':scope>button')];
   buttons.forEach((button,index)=>{
    for(const attribute of [...button.attributes])if(!next[index].hasAttribute(attribute.name))button.removeAttribute(attribute.name);
    for(const attribute of next[index].attributes)button.setAttribute(attribute.name,attribute.value);
    if(button.textContent!==next[index].textContent)button.replaceChildren(...next[index].childNodes);
   });
   group.className=replacement.className;replacement.replaceWith(group);
   if(group.contains(focused))focused.focus({preventScroll:true});
  }
 };
}
function inputMode(mode){if(root.dataset.input!==mode)root.dataset.input=mode;}
inputMode('pointer');
document.addEventListener('pointerdown',()=>inputMode('pointer'),true);
document.addEventListener('keydown',()=>inputMode('keyboard'),true);
document.addEventListener('click',event=>{
 const control=event.target.closest('#navigation .nav-link,.tabbar>button,.history-view-toggle>button,.overview-view-toggle>button,.clip-collection>button,.choice-switcher>button');
 if(control&&!control.disabled)schedule();
});
addEventListener('hashchange',schedule);
document.addEventListener('choice-switcher-sync',schedule);

// Keep native titles only for icon-only controls that need a plain-language
// label. Text controls use their visible copy and never grow a hover bubble.
export function setControlHint(owner,text){
 if(!owner)return;
 if(owner.dataset.tooltipNeeded==='true')owner.title=text;
 else owner.removeAttribute('title');
}
addEventListener('resize',()=>{snapNext=true;schedule();});
reducedPreference.addEventListener('change',()=>{if(reducedPreference.matches)for(const state of indicators.values())state.motion.finish();schedule();});
addEventListener('pagehide',()=>{for(const state of indicators.values())state.motion.stop();indicators.clear();for(const observer of observed.values())observer.disconnect();observed.clear();});
addEventListener('pageshow',schedule);
