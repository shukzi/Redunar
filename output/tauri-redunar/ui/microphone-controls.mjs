// Generation checks cover late starts, polls, audio reads and navigation.
export function microphoneTestClient({call, changed, schedule=setTimeout, unschedule=clearTimeout}) {
  let generation=0, id=null, timer=null;
  async function cancel() {
    generation++;unschedule(timer);timer=null;
    const previous=id;id=null;
    if(previous!==null)await call('stop_microphone_test',{id:previous}).catch(()=>{});
  }
  async function start() {
    const current=generation+1;
    await cancel();
    if(current!==generation)return;
    changed({phase:'starting'});
    try {
      const status=await call('start_microphone_test');
      if(current!==generation){await call('stop_microphone_test',{id:status.id}).catch(()=>{});return;}
      id=status.id;
      async function poll(status) {
        if(current!==generation)return;
        changed(status);
        if(status.phase==='ready') {
          const bytes=await call('microphone_test_audio',{id});
          if(current===generation)changed({...status,bytes});
        } else if(['starting','recording'].includes(status.phase)) {
          timer=schedule(()=>{
            void call('microphone_test_status',{id}).then(poll).catch(failed);
          },150);
        }
      }
      function failed(error) { if(current===generation)changed({phase:'failed',message:String(error)}); }
      await poll(status);
    } catch(error) { if(current===generation)changed({phase:'failed',message:String(error)}); }
  }
  return {start,cancel};
}

const signed=value=>`${value>0?'+':''}${value} dB`;
const idleStatus='6-second check · not saved';
const playbackTime=value=>{const seconds=Math.max(0,Math.floor(Number.isFinite(value)?value:0));return `${Math.floor(seconds/60)}:${String(seconds%60).padStart(2,'0')}`;};
export function microphoneControlsMarkup(gain=0, icon=()=> '') {
  const level=Number.isInteger(gain)&&gain>=-12&&gain<=24?gain:0;
  return `<section class="microphone-controls" aria-label="Microphone recording level">
    <div class="microphone-gain-label">Microphone recording<small id="microphone-gain-help">Adjust your voice in saved clips.<br>Other apps stay unchanged.</small></div>
    <div class="microphone-fields">
      <div class="microphone-gain-slider"><label for="microphone-gain"><span>Voice level</span><output for="microphone-gain">${signed(level)}</output></label><input id="microphone-gain" type="range" min="-12" max="24" step="1" value="${level}" aria-label="Microphone recording level" aria-describedby="microphone-gain-help microphone-test-status"></div>
      <div class="microphone-meter-wrap"><div class="microphone-meter-label"><span>Voice meter</span><span data-microphone-peak>Start a test to check</span></div><div class="microphone-meter" role="meter" aria-label="Recorded voice level" aria-valuemin="0" aria-valuemax="100" aria-valuenow="0" aria-valuetext="No measurement"><i></i></div></div>
      <div class="microphone-test-row"><button type="button" class="button secondary" data-microphone-test>Test microphone</button><button type="button" class="text-button" data-microphone-stop hidden>Stop test</button><span class="microphone-protection" title="Always on. Limits loud peaks in your recorded voice to help prevent distortion.">${icon('shieldCheck')}<span>Peak protection</span></span></div>
      <p class="microphone-test-status" id="microphone-test-status" role="status">${idleStatus}</p>
      <div class="microphone-playback" hidden>
        <button type="button" class="icon-button" data-microphone-play aria-label="Play microphone test" aria-pressed="false" disabled>${icon('play')}</button>
        <div class="microphone-test-track"><div class="microphone-meter-label"><span>Test recording</span><span data-microphone-time>0:00 / 0:00</span></div><input type="range" min="0" max="0" value="0" step="0.01" data-microphone-seek aria-label="Microphone test position" disabled></div>
        <button type="button" class="text-button" data-microphone-clear>Clear</button>
      </div>
      <audio preload="metadata" aria-label="Microphone test playback" hidden></audio>
    </div>
  </section>`;
}

export function microphoneControls({call, saved, locked, enabled, live, updateSlider, icon=()=> '', errorText=String}) {
  let root=null, saving=false, testing=false, starting=false, objectUrl=null, liveTimer=null, expiryTimer=null, view=0, suspended=false, confirmed=0;
  function status(text,error=false) { if(!root)return;const element=root.querySelector('.microphone-test-status');element.textContent=text;element.dataset.error=String(error); }
  function meter(levels) {
    if(!root)return;
    const peak=levels&&Number.isFinite(levels.peak)?Math.max(0,Math.min(1,levels.peak)):null;
    const value=peak===null?0:Math.round(peak*100);
    const meter=root.querySelector('[role="meter"]');meter.setAttribute('aria-valuenow',String(value));
    meter.setAttribute('aria-valuetext',peak===null?'No measurement':`${value} percent${levels.limiting?', peak protection active':''}`);
    meter.querySelector('i').style.width=value+'%';
    root.querySelector('[data-microphone-peak]').textContent=peak===null?(objectUrl?'Test finished':'Start a test to check'):levels.limiting?'Peak protection':peak===0?'Silent':Math.max(-60,Math.round(20*Math.log10(peak)))+' dBFS';
  }
  function paintPlayback() {
    if(!root)return;
    const audio=root.querySelector('audio'),button=root.querySelector('[data-microphone-play]'),seek=root.querySelector('[data-microphone-seek]');
    const duration=Number.isFinite(audio.duration)?audio.duration:0;
    button.disabled=!objectUrl||audio.readyState<1;
    const playing=String(!audio.paused&&!audio.ended);
    if(button.getAttribute('aria-pressed')!==playing){button.setAttribute('aria-pressed',playing);button.setAttribute('aria-label',playing==='true'?'Pause microphone test':'Play microphone test');button.innerHTML=icon(playing==='true'?'pause':'play');}
    seek.disabled=!objectUrl||duration<=0;seek.max=duration;seek.value=audio.currentTime||0;updateSlider(seek);
    root.querySelector('[data-microphone-time]').textContent=`${playbackTime(audio.currentTime)} / ${playbackTime(duration)}`;
  }
  function clearAudio() {
    clearTimeout(expiryTimer);expiryTimer=null;
    const audio=root?.querySelector('audio');
    if(audio){audio.pause();audio.removeAttribute('src');audio.load();root.querySelector('.microphone-playback').hidden=true;}
    if(objectUrl){URL.revokeObjectURL(objectUrl);objectUrl=null;}
    paintPlayback();
  }
  function update() {
    if(!root||!root.isConnected)return;
    const unavailable=locked()||!enabled()||saving;
    root.querySelector('#microphone-gain').disabled=unavailable||testing||starting;
    root.querySelector('[data-microphone-test]').disabled=unavailable||testing||starting;
    root.querySelector('[data-microphone-stop]').hidden=!testing;
    root.querySelector('[data-microphone-test]').textContent=objectUrl?'Record again':'Test microphone';
    if(!live()||document.hidden||suspended){clearTimeout(liveTimer);liveTimer=null;if(!testing)meter(null);}
    else if(liveTimer===null&&!testing){
      const current=view;
      liveTimer=setTimeout(async()=>{
        try{const levels=await call('microphone_levels');if(current===view&&!testing)meter(levels);}catch{if(current===view)meter(null);}
        if(current===view){liveTimer=null;update();}
      },200);
    }
  }
  const test=microphoneTestClient({call,changed:state=>{
    if(!root)return;
    testing=['starting','recording'].includes(state.phase);starting=false;
    meter(state.levels);
    if(state.phase==='starting')status('Opening the selected microphone…');
    if(state.phase==='recording')status(`Speak normally — recording ${state.seconds_recorded} / 6 seconds.`);
    if(state.phase==='ready') {
      status('Test ready. Listen back to check your recorded voice.');
      if(state.bytes){
        if(!Array.isArray(state.bytes)||state.bytes.length>1_152_044){status('The microphone test returned invalid audio.',true);void test.cancel();}
        else{
          objectUrl=URL.createObjectURL(new Blob([new Uint8Array(state.bytes)],{type:'audio/wav'}));
          const audio=root.querySelector('audio');audio.src=objectUrl;root.querySelector('.microphone-playback').hidden=false;paintPlayback();
          expiryTimer=setTimeout(()=>{void stop();},60_000);
        }
      }
    }
    if(state.phase==='failed'||state.phase==='expired')status(state.message?errorText(state.message):'The microphone test is no longer available. Try again.',true);
    update();
  }});
  async function stop(preserveStarting=false) {
    const stopped=test.cancel();clearAudio();testing=false;if(!preserveStarting)starting=false;meter(null);
    status(idleStatus);update();await stopped;
  }
  return {
    mount(workspace) {
      view++;suspended=false;clearTimeout(liveTimer);liveTimer=null;void stop();
      root=workspace.querySelector('.microphone-controls');if(!root)return;
      const input=root.querySelector('#microphone-gain');if(!saving)confirmed=Number(input.value);
      input.addEventListener('input',()=>{root.querySelector('output').textContent=signed(Number(input.value));});
      input.addEventListener('change',async()=>{
        if(locked()||testing||starting||saving){input.value=confirmed;return;}
        const current=view;const gainDb=Number(input.value);saving=true;update();
        await stop();
        try{
          const preferences=await call('set_microphone_gain',{gainDb});saved(preferences);confirmed=preferences.microphone_gain_db;
          if(root?.isConnected){const visible=root.querySelector('#microphone-gain');visible.value=confirmed;root.querySelector('output').textContent=signed(confirmed);updateSlider(visible);}
          if(current===view)status('Recording level saved for your next game session.');
        }
        catch(error){if(current===view){input.value=confirmed;root.querySelector('output').textContent=signed(confirmed);updateSlider(input);status(errorText(error),true);}}
        finally{saving=false;update();}
      });
      root.querySelector('[data-microphone-test]').addEventListener('click',async()=>{
        if(locked()||!enabled()||testing||starting)return;
        const current=view;starting=true;update();await stop(true);
        if(current!==view||!root?.isConnected||document.hidden||locked())return;
        starting=true;update();void test.start();
      });
      root.querySelector('[data-microphone-stop]').addEventListener('click',()=>{void stop();});
      root.querySelector('[data-microphone-clear]').addEventListener('click',()=>{void stop();});
      // Keep playback tied to this temporary view. Clearing, navigating or a
      // focus change must not revive audio after a pending play() settles.
      const mounted=root,audio=root.querySelector('audio');
      for(const event of ['loadedmetadata','timeupdate','play','pause','ended','emptied'])audio.addEventListener(event,()=>{if(root===mounted)paintPlayback();});
      audio.addEventListener('error',()=>{if(root===mounted&&objectUrl){clearAudio();status('The microphone test could not be played. Record another test.',true);update();}});
      root.querySelector('[data-microphone-play]').addEventListener('click',async()=>{
        if(!objectUrl)return;
        if(!audio.paused){audio.pause();return;}
        const current=view,url=objectUrl;
        try{if(audio.ended)audio.currentTime=0;await audio.play();if(current!==view||url!==objectUrl)audio.pause();}
        catch(error){if(current===view&&url===objectUrl)status(errorText(error),true);}
      });
      root.querySelector('[data-microphone-seek]').addEventListener('input',event=>{if(objectUrl&&Number.isFinite(audio.duration)){audio.currentTime=Number(event.target.value);paintPlayback();}});
      update();
    },
    update,
    suspend:()=>{view++;suspended=true;clearTimeout(liveTimer);liveTimer=null;return stop();},
    resume:()=>{suspended=false;update();},
  };
}
