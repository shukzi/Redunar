// A listening preference only. Never pass it to recording, trimming or export.
const key = 'redunar.playback-volume.v1';
export const playbackLevel = value => typeof value === 'number' && Number.isFinite(value) ? Math.max(0, Math.min(100, Math.round(value))) : 100;

export function playbackVolume({storage, getVideo, icon, updateSlider}) {
  let level = 100, previous = 100;
  try {
    const saved = storage?.getItem(key);
    if(saved !== null && saved !== undefined) {
      const value=JSON.parse(saved);
      level=playbackLevel(typeof value==='number'?value:value?.level);
      previous=playbackLevel(value?.previous)||100;
    }
  } catch { /* Restricted storage keeps a document-local preference. */ }
  if(level > 0) previous = level;
  let host=null, subscriptions=null, saveTimer=null, dirty=false;
  const apply=video=>{
    if(!video)return;
    if(Math.abs(video.volume-level/100)>.000001)video.volume=level/100;
    if(video.muted)video.muted=false;
  };
  function flush() {
    clearTimeout(saveTimer);saveTimer=null;
    if(!dirty)return;
    dirty=false;
    try{storage?.setItem(key,JSON.stringify({level,previous}));}catch{/* Playback still works without storage. */}
  }
  function saveLater() {
    dirty=true;clearTimeout(saveTimer);
    // Keep synchronous WebKit storage out of pointer/key input handlers. The
    // current in-memory value already applies to the player and next clip.
    saveTimer=setTimeout(flush,200);
  }
  function paint(syncRange=false) {
    const controls=host?.querySelector('.playback-volume');if(!controls)return;
    const input=controls.querySelector('input');
    if(syncRange){input.value=level;updateSlider(input);}
    const output=controls.querySelector('output'),text=level+'%';
    if(output.textContent!==text)output.textContent=text;
    const button=controls.querySelector('button'),muted=String(level===0);
    if(button.getAttribute('aria-pressed')!==muted){
      button.setAttribute('aria-label',level===0?'Unmute playback':'Mute playback');
      button.setAttribute('aria-pressed',muted);
      button.innerHTML=icon(level===0?'speakerMuted':'speaker');
    }
  }
  function change(value,syncRange=false) {
    const next=playbackLevel(value);
    if(next===level)return;
    level=next;if(level>0)previous=level;
    apply(getVideo());paint(syncRange);saveLater();
  }
  return {
    markup:()=>`<div class="playback-volume"><button type="button" class="icon-button playback-mute" aria-label="${level===0?'Unmute playback':'Mute playback'}" aria-pressed="${level===0}">${icon(level===0?'speakerMuted':'speaker')}</button><div class="playback-volume-field"><label for="clip-playback-volume">Playback volume <output for="clip-playback-volume">${level}%</output></label><input id="clip-playback-volume" type="range" min="0" max="100" step="1" value="${level}" style="--range-fill:${level}%" aria-describedby="playback-volume-help"></div></div>`,
    mount(root) {
      if(host!==root){
        subscriptions?.abort();subscriptions=new AbortController();host=root;
        const options={signal:subscriptions.signal};
        // Listen on the persistent workspace: clip selection/retry replaces
        // only the player subtree and must not leave its new controls inert.
        root.addEventListener('input',event=>{
          if(event.target.id==='clip-playback-volume')change(Number(event.target.value));
        },options);
        root.addEventListener('change',event=>{
          if(event.target.id==='clip-playback-volume')change(Number(event.target.value));
        },options);
        root.addEventListener('click',event=>{
          if(event.target.closest('.playback-mute'))change(level===0?previous:0,true);
        },options);
        root.ownerDocument.defaultView.addEventListener('pagehide',flush,options);
        root.ownerDocument.addEventListener('visibilitychange',()=>{if(root.ownerDocument.hidden)flush();},options);
      }
      paint(true);
    },
    apply,
  };
}
