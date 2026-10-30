// Diagnostic sampling only. Production UI never uses this frame loop.
window.addEventListener('DOMContentLoaded',()=>{
 const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
 const q=selector=>document.querySelector(selector);
 const wait=async predicate=>{for(let i=0;i<150;i++){if(predicate())return;await sleep(100);}throw Error('Fixture failed to become ready');};
 const summary=values=>{const sorted=[...values].sort((a,b)=>a-b);return sorted.length?{
  count:sorted.length,p50:sorted[Math.floor((sorted.length-1)*.5)],p95:sorted[Math.floor((sorted.length-1)*.95)],max:sorted.at(-1),
  mean:values.reduce((a,b)=>a+b,0)/values.length}:null;};
 (async()=>{
  await wait(()=>q('#clip-video')?.readyState>=2&&q('.clip-list')?.scrollHeight>q('.clip-list')?.clientHeight);
  await sleep(800);
  const video=q('#clip-video'),rail=q('.clip-list'),phases=[];
  video.muted=true;video.loop=true;video.pause();
  // Warm the complete rail once so decoding/loading does not confound the
  // comparison of paused scrolling and dropdown presentation.
  for(let i=0;i<10;i++){rail.scrollTop=i*(rail.scrollHeight-rail.clientHeight)/9;await sleep(70);}
  rail.scrollTop=0;await sleep(300);
  for(const label of ['paused-scroll','paused-dropdown','playing-scroll','playing-dropdown']){
   if(label.startsWith('playing'))await video.play();else video.pause();
   const intervals=[],dropdown=[],focus=[],before=video.currentTime;
   let previous=null,nextAction=0,start=performance.now();
   const clockBefore=await nativeRenderInvoke('render_clock',{enabled:true});
   await new Promise(resolve=>{
    const frame=now=>{
     if(previous!==null)intervals.push(now-previous);previous=now;
     focus.push(document.hasFocus()&&!document.hidden);
     if(label.endsWith('scroll'))rail.scrollTop=(1-Math.cos((now-start)/260))*.5*(rail.scrollHeight-rail.clientHeight);
     else if(now>=nextAction){
      const trigger=q('#save-duration+.precision-select-trigger');
      const began=performance.now();trigger.click();
      dropdown.push({event_to_dom_ms:performance.now()-began,open:!!q('.precision-select-menu')});
      nextAction=now+190;
     }
     if(now-start<1600)requestAnimationFrame(frame);else resolve();
    };requestAnimationFrame(frame);
   });
   q('.precision-select-menu')&&q('#save-duration+.precision-select-trigger').click();
   const clockAfter=await nativeRenderInvoke('render_clock',{enabled:false});
   phases.push({label,raf_ms:summary(intervals),raf_per_second:1000/(summary(intervals)?.mean||Infinity),
    paint_ms:summary(clockAfter.paint_intervals_ms),focused:focus.every(Boolean)&&clockBefore.native_focused&&clockAfter.native_focused,
    display_backend:clockAfter.display_backend,sampled_frames:focus.length,dropdown,position_before:before,position_after:video.currentTime,paused:video.paused});
   await sleep(150);
  }
  video.pause();
  await nativeRenderInvoke('render_report',{data:{fixture:true,generated_native_data:true,phases,errors:bridge.errors,
   limitations:['Scripted rail movement and dropdown clicks; physical input latency is not measured','rAF and GTK after-paint are proxies, not physical display presentation','Hidden/unfocused samples are invalid for refresh-rate comparison']}});
 })().catch(async error=>{await nativeRenderInvoke('render_report',{data:{error:String(error),errors:window.bridge?.errors}});});
});
