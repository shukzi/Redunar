// Executed by the isolated Tauri example against the production frontend.
location.hash = 'replay';
const states = [];
let ticks = 0;
let metadata = null, invalidMetadataRejected = false;
const playbackSeconds=Number(window.probePlaybackSeconds)||0;
const playback={started:false,stalls:0,maximumPosition:0,events:[],wallSeconds:0};
let playAt=null, previousSample=null, startedPlayingAt=null;
window.__TAURI_INTERNALS__.invoke('replay_clips').then(async clips=>{
 metadata=await window.__TAURI_INTERNALS__.invoke('clip_metadata',{fileName:clips[0].file_name});
 try{await window.__TAURI_INTERNALS__.invoke('clip_metadata',{fileName:'../outside.mkv'});}catch{invalidMetadataRejected=true;}
}).catch(error=>states.push({jsError:String(error)}));
const started = performance.now();
window.addEventListener('securitypolicyviolation', event => states.push({csp: event.effectiveDirective}));
window.addEventListener('error', event => states.push({jsError: event.message}));
const timer = setInterval(() => {
  const video = document.querySelector('#clip-video');
  if(playbackSeconds && playAt===null && video?.readyState>=2 && document.querySelector('#clip-filmstrip')?.dataset.frames==='8'){
    if(video.currentTime!==0)states.push({jsError:'Filmstrip changed player position'});
    video.muted=true;playAt=performance.now();
    for(const name of ['waiting','stalled','playing','pause','error'])video.addEventListener(name,()=>{playback.events.push({name,position:video.currentTime,elapsedMs:Math.round(performance.now()-playAt)});if(name==='playing'){playback.started=true;startedPlayingAt=performance.now();}});
    document.querySelector('#clip-play-toggle').click();
  }
  if(playAt!==null && video){
    const now=performance.now();
    playback.wallSeconds=(now-playAt)/1000;
    playback.maximumPosition=Math.max(playback.maximumPosition,video.currentTime);
    if(previousSample&&!video.paused&&!video.seeking&&startedPlayingAt!==null&&now-startedPlayingAt>500&&now-previousSample.at>150&&video.currentTime-previousSample.time<.05)playback.stalls++;
    previousSample={at:now,time:video.currentTime};
  }
  states.push({
    metadata, invalidMetadataRejected,
    elapsedMs: Math.round(performance.now() - started),
    ready: video?.readyState,
    network: video?.networkState,
    source: Boolean(video?.getAttribute('src')),
    preload: video?.preload,
    position: video?.currentTime,
    duration: Number.isFinite(video?.duration) ? video.duration : null,
    error: video?.error?.code,
    note: document.querySelector('#media-note')?.textContent,
    frames: document.querySelector('#clip-filmstrip')?.dataset.frames,
    playback:structuredClone(playback),
  });
  const prepared=document.querySelector('#clip-filmstrip')?.dataset.frames==='8'&&metadata&&invalidMetadataRejected;
  const played=playAt!==null&&(playback.wallSeconds>=playbackSeconds||(playback.maximumPosition>1&&video.paused&&playback.maximumPosition>=video.duration-.5));
  if (++ticks >= (playbackSeconds?4*(playbackSeconds+15):48) || (prepared&&(!playbackSeconds||played))) {
    clearInterval(timer);
    window.__TAURI_INTERNALS__.invoke('probe_report', {states});
  }
}, 250);
