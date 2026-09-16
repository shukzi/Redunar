// Executed by the isolated Tauri example against the production frontend.
location.hash = 'replay';
const states = [];
let ticks = 0;
let metadata = null, invalidMetadataRejected = false;
window.__TAURI_INTERNALS__.invoke('replay_clips').then(async clips=>{
 metadata=await window.__TAURI_INTERNALS__.invoke('clip_metadata',{fileName:clips[0].file_name});
 try{await window.__TAURI_INTERNALS__.invoke('clip_metadata',{fileName:'../outside.mkv'});}catch{invalidMetadataRejected=true;}
}).catch(error=>states.push({jsError:String(error)}));
const started = performance.now();
window.addEventListener('securitypolicyviolation', event => states.push({csp: event.effectiveDirective}));
window.addEventListener('error', event => states.push({jsError: event.message}));
const timer = setInterval(() => {
  const video = document.querySelector('#clip-video');
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
  });
  if (++ticks === 48 || (document.querySelector('#clip-filmstrip')?.dataset.frames === '8' && metadata && invalidMetadataRejected)) {
    clearInterval(timer);
    window.__TAURI_INTERNALS__.invoke('probe_report', {states});
  }
}, 250);
