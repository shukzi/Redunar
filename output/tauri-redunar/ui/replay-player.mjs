import {latestReplayRequest} from './replay-requests.mjs';
import {renderReplayFilmstrip} from './replay-filmstrip.mjs';
import {watchReplayLoading} from './replay-loading.mjs';
import {clamp,measurement} from './native-data.mjs';

// The player owns media listeners, preparation generations and trim gestures.
// Native code still authorizes paths and prepares the selected recording.
export function replayPlayer({query:$,selected:clip,call,native,applyVolume,
 updateMetadata:updateClipMetadata,updateSlider:updatePrecisionSlider,setControlHint,
 icon,spinner:clipExportSpinner,time,preciseTime,exporting,notify}) {
 let playerDuration = 0, playerPosition = 0, pendingSeek = null, trimStart = 0, trimEnd = 0;
 let stopFilmstrip = null;
 let mediaEvents = null;
 let timelineDrag = null, suppressTimelineClick = false, playbackFrame = null;
 let pendingSeekTask = Promise.resolve(), cancelPendingSeek = null, seekGeneration = 0, mediaActionGeneration = 0, playRequested = false, suppressPlayerToggleClickUntil = 0;
 let videoReady=false;
 const playbackRequests=latestReplayRequest(
  fileName=>call('clip_playback_path',{fileName}),
  ()=>call('cancel_clip_playback').catch(()=>{}),
 );
 const message=error=>error instanceof Error?error.message:String(error);
 function stopPlayback(){ const video=$('#clip-video');if(video)video.pause(); }
 function resetPlayback(){
  stopFilmstrip?.();stopFilmstrip=null;
  playbackRequests.invalidate();
  mediaEvents?.abort();mediaEvents=null;
  mediaActionGeneration++;seekGeneration++;
  if(cancelPendingSeek)cancelPendingSeek();
  cancelPendingSeek=null;pendingSeek=null;pendingSeekTask=Promise.resolve();
  stopPlayback();stopPlaybackTicker();
  const video=$('#clip-video');
  if(video){video.removeAttribute('src');video.load();}
  videoReady=false;playRequested=false;playerDuration=0;playerPosition=0;trimStart=0;trimEnd=0;timelineDrag=null;
 }
 function loadVideo() {
  const selected=clip(), video=$('#clip-video');
  if(!selected||!video)return;
  applyVolume(video);
  video.dataset.clipId=selected.id;
  videoReady=false;playRequested=false;updatePlayerUi();
  stopPlaybackTicker();
  playbackRequests.request(selected.file_name,url=>{
   if($('#clip-video')!==video)return;
   mediaEvents=new AbortController();
   const options={signal:mediaEvents.signal};
   video.addEventListener('loadeddata',()=>{videoReady=true;$('#media-state-overlay').hidden=true;const item=clip();if(item){item.duration=Number.isFinite(video.duration)?video.duration:null;updateClipMetadata(item);}const specs=$('#clip-video-specs');if(specs)specs.textContent=video.videoWidth&&video.videoHeight?`${video.videoWidth} × ${video.videoHeight}${clip()?.metadata?.fps?' · '+measurement(clip().metadata.fps,1)+' avg recorded FPS':''}`:'';playerDuration=Number.isFinite(video.duration)?video.duration:0;playerPosition=0;pendingSeek=null;pendingSeekTask=Promise.resolve();cancelPendingSeek=null;mediaActionGeneration++;playRequested=false;trimStart=0;trimEnd=playerDuration;updatePlayerUi();stopFilmstrip=renderReplayFilmstrip(video,$('#clip-filmstrip'),$('#clip-preview-status'));},{...options,once:true});
   video.addEventListener('timeupdate',()=>{if(handleTrimBoundary(video))return;if(pendingSeek===null&&Number.isFinite(video.currentTime))playerPosition=clamp(video.currentTime,trimStart,trimEnd||playerDuration);updatePlayerUi();},options);
   video.addEventListener('seeked',()=>{if(pendingSeek===null){playerPosition=video.currentTime;updatePlayerUi();}},options);
   video.addEventListener('play',()=>{playRequested=false;startPlaybackTicker();updatePlayerUi();},options);
   video.addEventListener('pause',()=>{playRequested=false;stopPlaybackTicker();updatePlayerUi();},options);
   video.addEventListener('ended',()=>{stopPlaybackTicker();if(trimEnd>0){playerPosition=trimEnd;updatePlayerUi();seekPlayer(trimStart);}else updatePlayerUi();},options);
   video.addEventListener('error',()=>failVideoLoad(video,'This video could not be played. Retry or open the selected clip externally.'),{...options,once:true});
   watchReplayLoading(video,mediaEvents.signal,()=>failVideoLoad(video,'The video did not finish loading. Retry or open the selected clip externally.'));
  $('#media-state-title').textContent='Loading local video…';
   video.src=url;
  },error=>failVideoLoad(video,message(error)));
 }
 function failVideoLoad(video,note){
  if($('#clip-video')!==video)return;
  resetPlayback();
  $('#media-state-overlay').hidden=false;$('#media-state-title').textContent='Clip could not load';$('#media-state-detail').textContent=note;
  $('#retry-clip').hidden=false;
  const status=$('#clip-preview-status');status.hidden=true;status.textContent='Frame previews will load when playback is ready.';
  updatePlayerUi();
 }
 const timelinePercent=value=>playerDuration>0?clamp(value/playerDuration*100,0,100):0;
 function stopPlaybackTicker(){if(playbackFrame!==null){cancelAnimationFrame(playbackFrame);playbackFrame=null;}}
 function handleTrimBoundary(video){
  if(video.paused||trimEnd<=0||!Number.isFinite(video.currentTime)||video.currentTime<trimEnd-0.03)return false;
  video.pause();playerPosition=trimEnd;updatePlayerUi();seekPlayer(trimStart);return true;
 }
 function playbackTick(){
  const video=$('#clip-video');
  if(!video||video.paused||video.ended){playbackFrame=null;return;}
  if(handleTrimBoundary(video)){playbackFrame=null;return;}
  if(pendingSeek===null&&Number.isFinite(video.currentTime)){playerPosition=clamp(video.currentTime,trimStart,trimEnd||playerDuration);updatePlayerUi();}
  playbackFrame=requestAnimationFrame(playbackTick);
 }
 function startPlaybackTicker(){stopPlaybackTicker();playbackFrame=requestAnimationFrame(playbackTick);}
 function seekPlayer(value){
  const video=$('#clip-video');
  if(!video||!playerDuration)return Promise.resolve();
  const target=clamp(Number(value)||0,0,playerDuration);
  const previousCancel=cancelPendingSeek;
  const generation=++seekGeneration;
  if(previousCancel)previousCancel();
  playerPosition=target;pendingSeek=target;
  pendingSeekTask=new Promise(resolve=>{
   let settled=false,timer=null,frameCallback=null;
   const cleanup=()=>{video.removeEventListener('seeked',onSeeked);video.removeEventListener('error',onError);if(timer!==null)clearTimeout(timer);if(frameCallback!==null&&video.cancelVideoFrameCallback)video.cancelVideoFrameCallback(frameCallback);};
   const finish=()=>{if(settled)return;settled=true;cleanup();if(generation===seekGeneration){cancelPendingSeek=null;playerPosition=target;pendingSeek=null;updatePlayerUi();}resolve();};
   const onSeeked=()=>{if(Math.abs(video.currentTime-target)<=.25)finish();};
   const onError=finish;
   const watchFrame=()=>{if(!video.requestVideoFrameCallback)return;frameCallback=video.requestVideoFrameCallback((_,metadata)=>{frameCallback=null;if(Math.abs(video.currentTime-target)<=.25&&Math.abs(metadata.mediaTime-target)<=3)finish();else if(!settled)watchFrame();});};
   cancelPendingSeek=finish;
   video.addEventListener('seeked',onSeeked);video.addEventListener('error',onError,{once:true});watchFrame();timer=setTimeout(finish,1500);
  });
  try{video.currentTime=target;}catch(error){if(cancelPendingSeek)cancelPendingSeek();return Promise.reject(error);}
  updatePlayerUi();return pendingSeekTask;
 }
 async function playPlayer(video,generation){
  if(!videoReady||video.readyState<1)return;
  const wanted=playerPosition<trimStart||playerPosition>=trimEnd-.03?trimStart:playerPosition;
  const settlingSeek=pendingSeek!==null;
  // Keep the first play() call inside the user gesture. Waiting for an
  // asynchronous seek first makes WebKitGTK treat the request as autoplay.
  const needsSeek=Math.abs(video.currentTime-wanted)>.25;
  if(needsSeek){try{video.currentTime=wanted;}catch(error){throw error;}}
  for(let attempt=0;attempt<2;attempt++){
   try{await video.play();break;}
   catch(error){if(error?.name!=='AbortError'||attempt===1||generation!==mediaActionGeneration)throw error;await new Promise(resolve=>setTimeout(resolve,80));}
  }
  if(generation!==mediaActionGeneration)return;
  if(settlingSeek)await pendingSeekTask;
  if(generation!==mediaActionGeneration)return;
  // WebKitGTK can resolve `seeked` before its paused decoder is ready to resume.
  // Reconcile a seek that was already in flight without losing the original gesture.
  if(Math.abs(video.currentTime-wanted)>.25)await seekPlayer(wanted);
  if(generation!==mediaActionGeneration)return;
 }
 function setTimelineValue(kind,value,preview=true){if(!videoReady||!playerDuration||exporting())return;const minimumGap=Math.min(.1,playerDuration);if(kind==='start'){trimStart=clamp(value,0,Math.max(0,trimEnd-minimumGap));if(playerPosition<trimStart)playerPosition=trimStart;}else if(kind==='end'){trimEnd=clamp(value,Math.min(playerDuration,trimStart+minimumGap),playerDuration);if(playerPosition>trimEnd)playerPosition=trimEnd;}else playerPosition=clamp(value,trimStart,trimEnd);updatePlayerUi();if(preview)seekPlayer(kind==='start'?trimStart:kind==='end'?Math.max(trimStart,trimEnd-.01):playerPosition);}
 function togglePlayback(){
  const video=$('#clip-video');
  if(!video||!videoReady||exporting())return;
  if(!video.paused||playRequested){
   playRequested=false;mediaActionGeneration++;video.pause();updatePlayerUi();return;
  }
  playRequested=true;
  const generation=++mediaActionGeneration;
  updatePlayerUi();
  playPlayer(video,generation).catch(error=>{
   if(generation!==mediaActionGeneration)return;
   playRequested=false;updatePlayerUi();
   notify(error?.name==='AbortError'?'Playback did not resume. Select that point again.':message(error));
  });
 }
 function updatePlayerUi(){
  const seek=$('#player-seek');if(seek){seek.min=String(trimStart);seek.max=String(trimEnd);seek.value=String(pendingSeek??playerPosition);seek.disabled=!videoReady||exporting();updatePrecisionSlider(seek);}
  const video=$('#clip-video');if(!video)return;
  const text=(selector,value)=>{const element=$(selector);if(element&&element.textContent!==value)element.textContent=value;};
  // Playback ticks also refresh static trim bounds; retain unchanged attributes.
  const attribute=(element,name,value)=>{if(element.getAttribute(name)!==value)element.setAttribute(name,value);};
  text('#clip-time',`${time(playerPosition)} / ${time(playerDuration||video.duration||0)}`);
  const center=$('#center-play');if(center){const hidden=!videoReady||!video.paused||playRequested;if(center.hidden!==hidden)center.hidden=hidden;center.disabled=exporting();}
  const reset=$('[data-action="reset-trim"]');if(reset)reset.disabled=!videoReady||exporting()||(trimStart===0&&trimEnd===playerDuration);
  const toggle=$('#clip-play-toggle');
  if(toggle){
   const paused=video.paused&&!playRequested,label=paused?'Play clip':'Pause clip';
   if(toggle.getAttribute('aria-label')!==label){toggle.innerHTML=icon(paused?'play':'pause');toggle.setAttribute('aria-label',label);setControlHint(toggle,label);}
   toggle.disabled=!videoReady||exporting();
  }
  const exportButton=$('#export-clip-button');
  if(exportButton){
   const adjusted=videoReady&&playerDuration>0&&(trimStart>0||trimEnd<playerDuration);
   const selectable=adjusted&&trimEnd-trimStart>=.1;
   exportButton.hidden=!adjusted;
   exportButton.disabled=!native||!selectable||exporting();
   const label=exporting()?'Exporting…':'Export';
   if(exportButton.textContent.trim()!==label)exportButton.innerHTML=`${exporting()?clipExportSpinner():icon('download')} ${label}`;
  }
  text('#trim-start-value',videoReady?preciseTime(trimStart):'—');
  text('#trim-end-value',videoReady?preciseTime(trimEnd):'—');
  text('#clip-selected-duration',videoReady?preciseTime(Math.max(0,trimEnd-trimStart)):'—');
  const ruler=$('#clip-time-ruler');
  if(ruler&&ruler.dataset.duration!==String(playerDuration)){
   ruler.dataset.duration=String(playerDuration);
   ruler.querySelectorAll('span').forEach((label,index)=>{label.textContent=playerDuration>0?(playerDuration<6?preciseTime(playerDuration*index/6):time(playerDuration*index/6)):'—';});
  }
  const start=$('#trim-start-bar'),end=$('#trim-end-bar'),playhead=$('#clip-playhead'),selection=$('#trim-selection'),timeline=$('#clip-timeline');
  for(const [element,value,min,max] of [[start,trimStart,0,Math.max(0,trimEnd-.1)],[end,trimEnd,Math.min(playerDuration,trimStart+.1),playerDuration]]){
   if(!element)continue;
   element.disabled=!videoReady||exporting();
   element.style.left=`${timelinePercent(value)}%`;
   attribute(element,'aria-valuemin',String(min));attribute(element,'aria-valuemax',String(max));
   attribute(element,'aria-valuenow',String(value.toFixed(2)));attribute(element,'aria-valuetext',preciseTime(value));
  }
  if(playhead)playhead.style.left=`${timelinePercent(playerPosition)}%`;
  if(timeline){
   attribute(timeline,'aria-disabled',String(!videoReady||exporting()));
   attribute(timeline,'aria-valuemax',String(playerDuration));attribute(timeline,'aria-valuenow',String(playerPosition.toFixed(2)));attribute(timeline,'aria-valuetext',preciseTime(playerPosition));
   timeline.style.setProperty('--trim-in',`${timelinePercent(trimStart)}%`);timeline.style.setProperty('--trim-out',`${timelinePercent(trimEnd)}%`);
  }
  if(selection){selection.style.left=`${timelinePercent(trimStart)}%`;selection.style.width=`${Math.max(0,timelinePercent(trimEnd)-timelinePercent(trimStart))}%`;}
 }

 function pointerTimelineValue(event){const timeline=$('#clip-timeline');if(!timeline||!playerDuration)return null;const rect=timeline.getBoundingClientRect();return clamp((event.clientX-rect.left)/Math.max(1,rect.width)*playerDuration,0,playerDuration);}
 function pointerDown(event){const timeline=event.target.closest('#clip-timeline');if(!timeline||!videoReady||exporting())return;event.preventDefault();suppressTimelineClick=true;timelineDrag=event.target.closest('[data-timeline]')?.dataset.timeline||'seek';timeline.setPointerCapture(event.pointerId);const value=pointerTimelineValue(event);if(value!==null)setTimelineValue(timelineDrag,value,false);}
 function pointerMove(event){if(!timelineDrag)return;const value=pointerTimelineValue(event);if(value!==null)setTimelineValue(timelineDrag,value,false);}
 function pointerUp(event){if(!timelineDrag)return;const kind=timelineDrag,value=pointerTimelineValue(event);timelineDrag=null;if(value!==null)setTimelineValue(kind,value,true);}
 function pointerCancel(){timelineDrag=null;suppressTimelineClick=false;}
 function keyDown(event){const target=event.target.closest('[data-timeline]');if(!target||!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;event.preventDefault();const kind=target.dataset.timeline,current=kind==='start'?trimStart:kind==='end'?trimEnd:playerPosition;let value=current+(event.key==='ArrowRight'?1:-1)*(event.shiftKey?5:.25);if(event.key==='Home')value=kind==='seek'?trimStart:kind==='end'?trimStart+.1:0;if(event.key==='End')value=kind==='seek'?trimEnd:kind==='start'?trimEnd-.1:playerDuration;setTimelineValue(kind,value,true);}

 function resetTrim(){
  if(!videoReady||exporting())return;
  trimStart=0;trimEnd=playerDuration;seekPlayer(0);updatePlayerUi();
 }
 function pause(){
  mediaActionGeneration++;playRequested=false;$('#clip-video')?.pause();updatePlayerUi();
 }
 function pointerToggle(event){
  const toggle=event.target.closest?.('[data-player="toggle"]');
  if(!toggle||toggle.disabled)return;
  event.preventDefault();suppressPlayerToggleClickUntil=performance.now()+500;togglePlayback();
 }
 function timelineClick(event){
  if(!event.target.closest('#clip-timeline'))return false;
  if(suppressTimelineClick){suppressTimelineClick=false;return true;}
  if(event.target.closest('.timeline-handle'))return false;
  const value=pointerTimelineValue(event);
  if(value!==null)setTimelineValue('seek',value,true);
  return true;
 }
 function transport(action){
  const video=$('#clip-video');
  if(!video)return;
  if(action==='toggle'){
   if(performance.now()<suppressPlayerToggleClickUntil){suppressPlayerToggleClickUntil=0;return;}
   togglePlayback();
  }
  if(action==='back')seekPlayer(clamp((pendingSeek??playerPosition)-5,trimStart,trimEnd));
  if(action==='forward'){
   const next=clamp((pendingSeek??playerPosition)+5,trimStart,trimEnd);
   if(next>=trimEnd-.01&&!video.paused)video.pause();
   seekPlayer(next);
  }
  updatePlayerUi();
 }

 return {
  load:loadVideo,reset:resetPlayback,update:updatePlayerUi,seek:seekPlayer,pause,resetTrim,
  pointerToggle,timelineClick,transport,pointerDown,pointerMove,pointerUp,pointerCancel,keyDown,
  range:()=>({ready:videoReady,duration:playerDuration,start:trimStart,end:trimEnd}),
 };
}
