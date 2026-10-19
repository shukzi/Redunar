// Bounded feedback for mounted desktop controls. No native state, live-data
// animation, wheel interception, polling or whole-document mutation observer.
const root=document.documentElement;
const reduced=()=>matchMedia('(prefers-reduced-motion: reduce)').matches;
const remembered=new Map(),observed=new Map();
let pending=false;
function schedule(){if(pending)return;pending=true;requestAnimationFrame(()=>{pending=false;refreshWorkspacePresentation();});}
function indicator(group,kind,active,key){
 if(!active)return;
 const box=active.getBoundingClientRect(),parent=group.getBoundingClientRect();
 if(!box.width||!box.height)return;
 const geometry=kind==='nav'?{x:0,y:box.top-parent.top+11,w:3,h:box.height-22}:
  kind==='tabs'?{x:box.left-parent.left-group.clientLeft,y:group.clientHeight-2,w:box.width,h:2}:
  {x:box.left-parent.left-group.clientLeft,y:box.top-parent.top-group.clientTop,w:box.width,h:box.height};
 const selection=active.getAttribute('href')||active.dataset.clipScope||active.textContent.trim();
 const prior=remembered.get(key);
 let plate=group.querySelector(':scope>.selection-indicator');
 if(!plate){plate=document.createElement('i');plate.className=`selection-indicator selection-indicator-${kind}`;plate.setAttribute('aria-hidden','true');group.append(plate);}
 if(kind==='segment'){plate.style.background='#242424';plate.style.borderColor='#444';plate.style.borderRadius=getComputedStyle(active).borderRadius;}
 plate.style.width=`${geometry.w}px`;plate.style.height=`${geometry.h}px`;plate.style.transform=`translate(${geometry.x}px,${geometry.y}px)`;
 group.dataset.selectionIndicator=kind;
 if(prior&&prior.selection!==selection&&!reduced()){
  plate.getAnimations().forEach(animation=>animation.cancel());
  plate.animate([{transform:`translate(${prior.x}px,${prior.y}px)`,width:`${prior.w}px`},{transform:`translate(${geometry.x}px,${geometry.y}px)`,width:`${geometry.w}px`}],{duration:190,easing:'cubic-bezier(.2,.8,.2,1)'});
 }
 remembered.set(key,{...geometry,selection});
 if(!observed.has(group)){
  const observer=new ResizeObserver(schedule);observer.observe(group);
  // Dirty markers can resize an individual tab without resizing its full-width
  // bar. Observe bounded controls too, including edits made without a click.
  for(const button of group.querySelectorAll(':scope>button'))observer.observe(button);
  observed.set(group,observer);
 }
}
export function refreshWorkspacePresentation(){
 for(const [group,observer] of observed)if(!group.isConnected){observer.disconnect();observed.delete(group);}
 const nav=document.querySelector('#navigation');if(nav)indicator(nav,'nav',nav.querySelector('.nav-link.active'),'navigation');
 document.querySelectorAll('.tabbar').forEach(group=>{
  const buttons=[...group.querySelectorAll(':scope>button')];
  indicator(group,'tabs',buttons.find(button=>button.classList.contains('active')),'tabs:'+buttons.map(button=>button.dataset.globalTab||button.dataset.libraryTab||button.dataset.overlayEditorTab||button.textContent.trim()).join('|'));
 });
 const clips=document.querySelector('.clip-collection');if(clips)indicator(clips,'segment',clips.querySelector('[aria-pressed=true]'),'clip-collection');
}
function inputMode(mode){if(root.dataset.input!==mode)root.dataset.input=mode;}
inputMode('pointer');
document.addEventListener('pointerdown',()=>{inputMode('pointer');hideHint();},true);
document.addEventListener('keydown',event=>{inputMode('keyboard');if(event.key==='Escape')hideHint();},true);
document.addEventListener('click',event=>{
 const control=event.target.closest('#navigation .nav-link,.tabbar>button,.clip-collection>button');
 if(control&&!control.disabled)schedule();
});
addEventListener('hashchange',()=>{hideHint();schedule();});

// One transient tooltip uses each control's existing label. It never replaces
// native focus or changes the action associated with the control.
const hintSelector='.icon-button[aria-label],.clip-library-actions button[aria-label],.sidebar-toggle,.clip-action-menu>summary,.button[title],[data-control-title],.nav-link';
let hintTimer=null,hintOwner=null,hint=null;
export function setControlHint(owner,text){
 if(!owner)return;
 if(owner.hasAttribute('data-control-title')){
  const changed=owner.dataset.controlTitle!==text;
  if(changed)owner.dataset.controlTitle=text;
  owner.removeAttribute('title');
  if(changed&&hintOwner===owner){hideHint();showHint(owner);}
 }else if(owner.title!==text)owner.title=text;
}
function hideHint(){
 clearTimeout(hintTimer);hintTimer=null;
 if(hintOwner&&hint){const ids=(hintOwner.getAttribute('aria-describedby')||'').split(' ').filter(id=>id&&id!==hint.id);if(ids.length)hintOwner.setAttribute('aria-describedby',ids.join(' '));else hintOwner.removeAttribute('aria-describedby');}
 hint?.remove();hint=null;hintOwner=null;
}
function showHint(owner){
 if(!owner.isConnected||owner.disabled)return;
 if(owner.matches('.nav-link')&&root.dataset.sidebar!=='collapsed'&&innerWidth>700)return;
 const text=owner.getAttribute('title')||owner.dataset.controlTitle||owner.getAttribute('aria-label');if(!text)return;
 if(owner.hasAttribute('title')){owner.dataset.controlTitle=owner.title;owner.removeAttribute('title');}
 hintOwner=owner;hint=document.createElement('div');hint.id='control-hint';hint.className='control-hint';hint.setAttribute('role','tooltip');hint.textContent=text;document.body.append(hint);
 owner.setAttribute('aria-describedby',((owner.getAttribute('aria-describedby')||'')+' '+hint.id).trim());
 const box=owner.getBoundingClientRect(),size=hint.getBoundingClientRect();
 hint.style.left=`${Math.max(8,Math.min(box.left+(box.width-size.width)/2,innerWidth-size.width-8))}px`;
 hint.style.top=`${box.bottom+size.height+16<innerHeight?box.bottom+8:Math.max(8,box.top-size.height-8)}px`;
}
document.addEventListener('pointerover',event=>{const owner=event.target.closest(hintSelector);if(!owner||owner.contains(event.relatedTarget))return;hideHint();hintTimer=setTimeout(()=>showHint(owner),400);});
document.addEventListener('pointerout',event=>{const owner=event.target.closest(hintSelector);if(owner&&!owner.contains(event.relatedTarget))hideHint();});
document.addEventListener('focusin',event=>{hideHint();const owner=event.target.closest(hintSelector);if(owner&&root.dataset.input==='keyboard')showHint(owner);});
document.addEventListener('focusout',hideHint);
document.addEventListener('scroll',hideHint,true);
document.addEventListener('visibilitychange',()=>{if(document.hidden)hideHint();});
addEventListener('resize',()=>{hideHint();schedule();});
