// Anchored surfaces keep their final layout throughout the visual transition.
// Closing callers release input immediately; only the inert surface remains.
const active=new Map(),installed=new WeakSet();
function release(panel,state){
  if(active.get(panel)!==state)return false;
  active.delete(panel);
  state.animation.onfinish=null;state.animation.cancel();
  panel.style.transformOrigin=state.origin;
  return true;
}
export function cancelPopupMotion(panel){
  const state=active.get(panel);
  if(state)release(panel,state);
}
function install(doc){
  if(installed.has(doc))return;
  installed.add(doc);
  const finish=()=>{for(const [panel,state] of active)if(panel.ownerDocument===doc&&release(panel,state))state.complete();};
  const preference=doc.defaultView.matchMedia('(prefers-reduced-motion: reduce)');
  preference.addEventListener('change',()=>{if(preference.matches)finish();});
  doc.defaultView.addEventListener('pagehide',finish);
}
export function setPopupExpanded(panel,expanded,{edge='top',immediate=false,onfinish=()=>{}}={}){
  const doc=panel.ownerDocument,view=doc.defaultView,previous=active.get(panel);
  install(doc);
  const painted=previous?view.getComputedStyle(panel):null;
  const first=painted?{opacity:painted.opacity,transform:painted.transform}:null;
  const origin=previous?.origin??panel.style.transformOrigin;
  cancelPopupMotion(panel);
  if(immediate||view.matchMedia('(prefers-reduced-motion: reduce)').matches||!panel.animate){onfinish();return;}
  const folded={opacity:0,transform:`translateY(${edge==='bottom'?2:-2}px) scaleY(.96)`};
  const unfolded={opacity:1,transform:'translateY(0px) scaleY(1)'};
  panel.style.transformOrigin=`center ${edge}`;
  const animation=panel.animate([first||(expanded?folded:unfolded),expanded?unfolded:folded],{
    duration:expanded?200:160,easing:expanded?'cubic-bezier(.16,1,.3,1)':'cubic-bezier(.4,0,1,1)',fill:'both',
  });
  animation.id=expanded?'dropdown-open':'dropdown-close';
  const state={animation,origin,complete:onfinish};active.set(panel,state);
  animation.onfinish=()=>{if(release(panel,state))onfinish();};
}
