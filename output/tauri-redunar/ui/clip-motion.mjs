// Local presentation only; inventory, playback and selection keep their owners.
const running = new Set();
const reduced = typeof matchMedia==='function'?matchMedia('(prefers-reduced-motion: reduce)'):null;
const prefersReduced = () => typeof matchMedia==='function'&&matchMedia('(prefers-reduced-motion: reduce)').matches;
reduced?.addEventListener('change',event=>{if(event.matches)for(const animation of running)animation.finish();});
function animate(element,frames,duration) {
 if(prefersReduced()||!element.animate)return;
 const animation=element.animate(frames,{duration,easing:'cubic-bezier(.2,0,0,1)'});
 running.add(animation);
 animation.finished.catch(()=>{}).finally(()=>running.delete(animation));
}
export function clipPositions(rail) {
 if(prefersReduced())return new Map();
 const bounds=rail.getBoundingClientRect();
 return new Map(Array.from(rail.querySelectorAll('[data-clip],[data-recent-clip]')).flatMap(button=>{
  const rect=(button.closest('.clip-card')||button).getBoundingClientRect();
  return rect.bottom>=bounds.top&&rect.top<=bounds.bottom?[[button.dataset.clip||button.dataset.recentClip,rect.top]]:[];
 }));
}
export function animateClipArrival(rail,before,newIds) {
 if(!newIds.size||prefersReduced())return;
 const bounds=rail.getBoundingClientRect();
 for(const button of rail.querySelectorAll('[data-clip],[data-recent-clip]')) {
  const id=button.dataset.clip||button.dataset.recentClip,card=button.closest('.clip-card')||button,rect=card.getBoundingClientRect();
  if(rect.bottom<bounds.top||rect.top>bounds.bottom)continue;
  if(newIds.has(id))animate(card,[{opacity:0},{opacity:1}],180);
  else if(before.has(id)) {
   const offset=before.get(id)-rect.top;
   if(Math.abs(offset)>1)animate(card,[{transform:`translateY(${offset}px)`},{transform:'none'}],220);
  }
 }
}
export function mountClipThumbnail(target,source,alt='',onError=()=>{}) {
 if(!target||target.querySelector('img'))return;
 const image=document.createElement('img');
 image.alt=alt;image.decoding='async';image.dataset.thumbnailPending='';
 const failed=()=>{image.remove();onError();};
 image.addEventListener('error',failed,{once:true});
 image.addEventListener('load',async()=>{
  try { await image.decode(); } catch { failed();return; }
  if(!image.isConnected)return;
  target.querySelector('.icon')?.remove();
  delete image.dataset.thumbnailPending;
  animate(image,[{opacity:0},{opacity:1}],160);
 },{once:true});
 image.src=source;target.prepend(image);
}
