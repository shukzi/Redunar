// Original native SVG motion, implemented from visible interaction behavior.
// Lucide artwork and its notices remain separate from these motion profiles.
const active=new Map(),running=new Map(),installed=new WeakSet();
const reduced=node=>node?.ownerDocument?.defaultView?.matchMedia('(prefers-reduced-motion: reduce)').matches;
function animate(node,frames,options={}) {
  if(!node?.animate||reduced(node))return null;
  active.get(node)?.cancel();
  const animation=node.animate(frames,{duration:600,easing:'cubic-bezier(.25,.1,.25,1)',...options});
  active.set(node,animation);
  animation.onfinish=animation.oncancel=()=>{if(active.get(node)===animation)active.delete(node);};
  return animation;
}
function tween(duration,update) {
  let frame,finished=false,resolve;
  const start=performance.now(),promise=new Promise(done=>resolve=done);
  const tick=now=>{
    if(finished)return;
    const progress=Math.min(1,(now-start)/duration);
    update(Math.sin(Math.PI*progress)**2);
    if(progress===1){finished=true;resolve();}else frame=requestAnimationFrame(tick);
  };
  frame=requestAnimationFrame(tick);
  return {finished:promise,cancel(){finished=true;cancelAnimationFrame(frame);resolve();}};
}
function cancelIcon(svg) {
  const job=running.get(svg);if(!job)return;
  running.delete(svg);
  job.animations.forEach(animation=>{
    for(const [node,value] of active)if(value===animation)active.delete(node);
    animation.cancel();
  });
  job.cleanup.forEach(clean=>clean());
}
function settle() {
  for(const svg of running.keys())cancelIcon(svg);
  for(const animation of active.values())animation.cancel();
  active.clear();

}
function playIcon(svg) {
  // Finish an active gesture instead of restarting it on internal pointer moves
  // or the focus event that follows a click. This avoids abrupt jumps.
  if(!svg||running.has(svg)||reduced(svg)||svg.classList.contains('updater-spinner')||svg.classList.contains('clip-export-spinner'))return;
  const group=svg.querySelector('.icon-strokes');if(!group)return;
  const parts=[...group.children],kind=svg.dataset.lucide;
  const job={animations:[],cleanup:[]};running.set(svg,job);
  const add=(node,frames,options)=>{const animation=animate(node,frames,options);if(animation)job.animations.push(animation);};
  const origin=(node,point='12px 12px')=>{node.style.transformBox='view-box';node.style.transformOrigin=point;};
  const gesture=(node,pose,{duration=600,delay=0,point='12px 12px'}={})=>{
    origin(node,point);
    add(node,[{transform:'none'},{transform:pose,offset:.42},{transform:'none'}],{duration,delay,easing:'cubic-bezier(.4,0,.2,1)'});
  };
  const draw=(nodes=parts)=>nodes.forEach((part,index)=>{
    part.setAttribute('pathLength','1');part.setAttribute('stroke-dasharray','1 1');
    const reverse=kind==='check'||kind==='file-text'||(kind==='plus'&&index===0);
    add(part,[{strokeDashoffset:reverse?-1:1,opacity:0},{strokeDashoffset:0,opacity:1}],{duration:560,delay:index*28,easing:'cubic-bezier(.33,0,.2,1)',fill:'backwards'});
  });
  if(kind==='refresh-cw'||kind==='rotate-ccw'){
    origin(group);add(group,[{transform:'rotate(0deg)'},{transform:`rotate(${kind==='rotate-ccw'?'-':''}360deg)`}],{duration:720,easing:'cubic-bezier(.4,0,.2,1)'});
  } else if(kind==='settings'){
    origin(group);add(group,[{transform:'rotate(0deg)'},{transform:'rotate(180deg)'}],{duration:760,easing:'cubic-bezier(.4,0,.2,1)'});
  } else if(kind==='library-big'){
    origin(parts[2],'17.6px 20.96px');
    add(parts[2],[{transform:'rotate(0deg)'},{transform:'rotate(20deg)',offset:.5},{transform:'rotate(0deg)'}],{duration:1000,easing:'ease-in-out'});
  } else if(kind==='group'){
    origin(parts[0]);
    const poses=(rest,inward,overshoot)=>[{transform:rest},{transform:inward,offset:.36},{transform:overshoot,offset:.7},{transform:rest}];
    add(parts[0],poses('scale(1)','scale(.85)','scale(1.03)'),{duration:500,easing:'cubic-bezier(.37,0,.63,1)'});
    for(let index=1;index<3;index++){
      const sign=index===1?1:-1;
      add(parts[index],poses('translate(0px,0px)',`translate(${sign*.75}px,${sign*.75}px)`,`translate(${-sign*.2}px,${-sign*.2}px)`),{duration:500,easing:'cubic-bezier(.37,0,.63,1)'});
    }
  } else if(kind==='panel-left-close'){
    const arrow=group.querySelector('.sidebar-chevron-motion')||parts[2];
    const direction=svg.ownerDocument.documentElement.dataset.sidebar==='collapsed'?1:-1;
    gesture(arrow,`translateX(${direction*2}px)`,{duration:600});
  } else if(kind==='sliders-horizontal'){
    const original=parts.map(part=>part.getAttribute('d'));
    job.cleanup.push(()=>parts.forEach((part,index)=>part.setAttribute('d',original[index])));
    job.animations.push(tween(780,amount=>{
      const top=14-6*amount,middle=8+8*amount,bottom=16-6*amount;
      const paths=[`M${top-4} 5H3`,`M${bottom-4} 19H3`,`M${top} 3v4`,`M${bottom} 17v4`,`M21 12H${middle+4}`,`M21 19H${bottom}`,`M21 5H${top}`,`M${middle} 10v4`,`M${middle} 12H3`];
      parts.forEach((part,index)=>part.setAttribute('d',paths[index]));
    }));
  } else if(kind==='folder'){
    const flap=parts[0].cloneNode(true);group.append(flap);job.cleanup.push(()=>flap.remove());
    origin(flap,'2px 20px');
    add(flap,[{transform:'none',opacity:0},{transform:'skewX(-12deg) scaleY(.82)',opacity:1,offset:.42},{transform:'none',opacity:0}],{duration:680,easing:'cubic-bezier(.4,0,.2,1)'});
  } else if(kind==='folder-open'){
    origin(parts[1],'14px 20px');
    add(parts[1],[{transform:'none'},{transform:'scaleY(.8) skewX(-6deg)',offset:.5},{transform:'none'}],{duration:700,easing:'ease-in-out'});
  } else if(kind==='pencil'){
    gesture(group,'translate(-2px,2px)',{duration:600});
  } else if(kind==='shrink'){
    const vectors=[[-2,-2],[2,-2],[-2,2],[2,2]];
    parts.forEach((part,index)=>gesture(part,`translate(${vectors[index][0]}px,${vectors[index][1]}px)`,{duration:600}));
  } else if(kind==='external-link'){
    gesture(group,'translate(2px,-2px)',{duration:600});
  } else if(kind==='download'){
    [parts[0],parts[2]].forEach(part=>gesture(part,'translateY(3px)',{duration:620}));
  } else if(kind==='trash'){
    const original=parts.map(part=>({d:part.getAttribute('d'),transform:part.style.transform}));
    job.cleanup.push(()=>parts.forEach((part,index)=>{if(original[index].d!==null)part.setAttribute('d',original[index].d);part.style.transform=original[index].transform;}));
    job.animations.push(tween(700,amount=>{
      parts.forEach((part,index)=>part.style.transform=`translateY(${[0,2].includes(index)?-amount:amount}px)`);
      const top=6+2*amount;
      parts[1].setAttribute('d',`M19 ${top}v${20-top}c0 1-1 2-2 2H7c-1 0-2-1-2-2V${top}`);
    }));
  } else if(kind==='volume-2'){
    parts.slice(1).forEach((part,index)=>add(part,[{opacity:1},{opacity:.15,offset:.4},{opacity:1}],{duration:620,delay:index*90}));
  } else if(kind==='copy'){
    gesture(parts[0],'translate(2px,2px)',{duration:600});
    gesture(parts[1],'translate(-1px,-1px)',{duration:600});
  } else if(kind==='ellipsis'){
    parts.forEach((part,index)=>gesture(part,'translateY(-2.5px)',{duration:480,delay:index*90}));
  } else if(kind==='clock-3'){
    const hand=parts.find(part=>part.tagName.toLowerCase()==='path');
    origin(hand);add(hand,[{transform:'rotate(0deg)'},{transform:'rotate(360deg)'}],{duration:800,easing:'cubic-bezier(.4,0,.2,1)'});
  } else if(kind==='search'){
    gesture(group,'translate(1px,-1px) rotate(-8deg)',{duration:620});
  } else if(kind==='star'){
    gesture(group,'rotate(-12deg) scale(1.08)',{duration:620});
  } else if(kind==='arrow-right'){
    gesture(group,'translateX(3px)',{duration:560});
  } else if(kind==='play'){
    gesture(group,'scale(1.1)',{duration:560});
  } else if(kind==='pause'){
    parts.forEach(part=>gesture(part,`translateX(${Number(part.getAttribute('x'))<12?2:-2}px)`,{duration:600}));
  } else if(kind==='clapperboard'){
    gesture(group,'scale(.96) rotate(-3deg)',{duration:680});
    parts.slice(0,3).forEach(part=>gesture(part,'rotate(-8deg)',{point:'3px 11px',duration:680}));
  } else draw();
  Promise.allSettled(job.animations.map(animation=>animation.finished)).then(()=>{
    if(running.get(svg)!==job)return;
    running.delete(svg);job.cleanup.forEach(clean=>clean());
  });
}
export function installIconMotion(doc=document) {
  if(installed.has(doc))return;
  installed.add(doc);
  const controller=new AbortController(),options={signal:controller.signal};
  const greet=event=>{
    const control=event.target.closest?.('button,a,summary,label.search');
    if(!control||control.matches(':disabled,[aria-disabled="true"]')||control.closest('[inert]')||control.contains(event.relatedTarget))return;
    if(!control.matches('#navigation .nav-link,.settings-link,[data-sidebar-toggle],.clip-library-actions button,.clip-edit-actions button:not([data-action="favorite-selected-clip"]),.history-catalog-heading button[data-action="reload-history"],.update-refresh'))return;
    if(event.type==='focusin'&&!event.target.matches(':focus-visible'))return;
    const svg=control.querySelector('.copy-button-symbol[data-copied="true"] .copy-glyph-confirmed')||control.querySelector('[data-lucide]');
    playIcon(svg);
  };
  doc.addEventListener('pointerover',greet,options);
  doc.addEventListener('focusin',greet,options);
  const preference=doc.defaultView.matchMedia('(prefers-reduced-motion: reduce)');
  preference.addEventListener('change',()=>{if(preference.matches)settle();},options);
  doc.defaultView.addEventListener('pagehide',event=>{settle();if(!event.persisted){controller.abort();installed.delete(doc);}},{once:true});
}
