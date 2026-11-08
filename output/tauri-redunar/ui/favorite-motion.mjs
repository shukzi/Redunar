// Original implementation of the documented press and particle behavior.
// Persistence and the acknowledged favorite state remain owned by the caller.
import {icon} from './icons.mjs';
import {springMotion} from './spring-motion.mjs';
const selector='[data-favorite-clip],[data-action="favorite-selected-clip"]';
const installed=new WeakSet(),motions=new WeakMap(),moving=new Set(),effects=new Map();
const reduced=button=>button.ownerDocument.defaultView.matchMedia('(prefers-reduced-motion: reduce)').matches;
function scaleTo(button,scale,immediate=false){
  let motion=motions.get(button);
  if(!motion){
    motion=springMotion({scale:1},value=>{
      if(!button.isConnected){motion?.stop();moving.delete(button);return;}
      button.style.transform=`scale(${value.scale})`;
    },{stiffness:550,damping:30,epsilon:.00005,velocityEpsilon:.001,onRest:()=>{
      moving.delete(button);
      if(motion.snapshot().target.scale===1)button.style.removeProperty('transform');
    }});
    motions.set(button,motion);
  }
  motion.to({scale},{immediate});if(motion.running)moving.add(button);
}
function cancelBurst(button){
  const job=effects.get(button);if(!job)return;
  effects.delete(button);job.events.abort();job.animations.forEach(animation=>animation.cancel());job.root.remove();
}
export function cancelFavoriteBurst(button,key=button?.dataset?.favoriteClip){
  // Reloading/filtering can replace a rail button before its portal expires.
  // Cancel by clip identity as well as node identity when the state changes.
  for(const [source,job] of effects)if(source===button||(key&&job.key===key&&source.ownerDocument===button.ownerDocument))cancelBurst(source);
}
export function animateFavorite(button,origin,key=button?.dataset?.favoriteClip){
  if(!button||(!button.isConnected&&!origin))return;
  cancelFavoriteBurst(button,key);
  if(reduced(button))return;
  const root=button.ownerDocument.createElement('span');
  root.className='favorite-burst';root.setAttribute('aria-hidden','true');
  const events=new AbortController(),job={root,animations:[],events,key},followsButton=button.isConnected;effects.set(button,job);
  // The clip rail scrolls independently. A short-lived portal prevents the
  // first row from clipping hearts, and follows scroll/resize until it ends.
  const position=()=>{
    if(followsButton&&!button.isConnected){cancelBurst(button);return;}
    const box=followsButton?button.getBoundingClientRect():origin;root.style.left=`${box.left+box.width/2}px`;root.style.top=`${box.top}px`;
  };
  position();button.ownerDocument.body.append(root);
  button.ownerDocument.addEventListener('scroll',position,{capture:true,passive:true,signal:events.signal});
  button.ownerDocument.defaultView.addEventListener('resize',position,{signal:events.signal});
  button.ownerDocument.defaultView.addEventListener('hashchange',()=>cancelBurst(button),{signal:events.signal});
  for(let index=0;index<6;index++){
    const particle=button.ownerDocument.createElement('span'),angle=(index/6-.25)*Math.PI*2;
    const x=Math.cos(angle)*30,y=Math.sin(angle)*30;
    particle.className='favorite-particle';particle.innerHTML=icon('heart','favorite-particle-heart');root.append(particle);
    const pose=(distance,scale)=>`translate(calc(-50% + ${x*distance}px),calc(-50% + ${y*distance}px)) scale(${scale})`;
    job.animations.push(particle.animate([
      {transform:pose(0,0),opacity:0},
      {transform:pose(.65,1),opacity:1,offset:.4},
      {transform:pose(1,0),opacity:0},
    ],{duration:800,delay:index*50,easing:'ease-out',fill:'both'}));
  }
  Promise.allSettled(job.animations.map(animation=>animation.finished)).then(()=>{
    if(effects.get(button)!==job)return;
    effects.delete(button);events.abort();root.remove();
  });
}
export function installFavoriteMotion(doc=document){
  if(installed.has(doc))return;installed.add(doc);
  const pressed=new Set(),controller=new AbortController(),options={signal:controller.signal};
  const control=event=>event.target.closest?.(selector);
  const press=event=>{
    if(event.type==='keydown'&&(!['Enter',' '].includes(event.key)||event.repeat))return;
    if(event.type==='pointerdown'&&event.button!==0)return;
    const button=control(event);if(!button||button.disabled||reduced(button))return;
    pressed.add(button);scaleTo(button,.97);
  };
  const release=()=>{for(const button of pressed)scaleTo(button,1,reduced(button));pressed.clear();};
  doc.addEventListener('pointerdown',press,options);doc.addEventListener('keydown',press,options);
  doc.addEventListener('pointerup',release,options);doc.addEventListener('pointercancel',release,options);
  doc.addEventListener('keyup',event=>{if(['Enter',' '].includes(event.key))release();},options);
  const settle=()=>{
    for(const button of new Set([...pressed,...moving]))scaleTo(button,1,true);
    pressed.clear();moving.clear();for(const button of effects.keys())cancelBurst(button);
  };
  const preference=doc.defaultView.matchMedia('(prefers-reduced-motion: reduce)');
  preference.addEventListener('change',()=>{if(preference.matches)settle();},options);
  doc.addEventListener('visibilitychange',()=>{if(doc.hidden)settle();},options);
  doc.defaultView.addEventListener('blur',release,options);
  doc.defaultView.addEventListener('pagehide',event=>{settle();if(!event.persisted){controller.abort();installed.delete(doc);}},{once:true});
}
