"""Production UI with synthetic audio/video and fake native microphone commands.

Run after building the frontend, under Xvfb with
REDUNAR_PRIVATE_DESKTOP_FIXTURE=1. Never opens a microphone or the native app.
"""
import io
import os
import wave
import subprocess

if os.environ.get('REDUNAR_PRIVATE_DESKTOP_FIXTURE') != '1':
    raise SystemExit('Use the isolated desktop fixture under Xvfb')
os.environ['GDK_BACKEND'] = 'x11'
os.environ['GDK_SCALE'] = '1'
os.environ['GDK_DPI_SCALE'] = '1'

import webview_fixture as harness
from webview_fixture import *

ARTIFACTS = NATIVE.parents[1] / '.redunar-build/reports/audio-controls'
ARTIFACTS.mkdir(parents=True, exist_ok=True)
harness.ARTIFACTS = ARTIFACTS
movie = ARTIFACTS / 'synthetic.mp4'
subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-f', 'lavfi',
                '-i', 'testsrc2=size=640x360:rate=10', '-t', '3', '-c:v', 'libx264',
                '-pix_fmt', 'yuv420p', str(movie)], check=True)
original_get = Handler.do_GET
def media_get(self):
    if self.path == '/__audio-controls-synthetic.mp4':
        payload = movie.read_bytes()
        self.send_response(200)
        self.send_header('Content-Type', 'video/mp4')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
    else:
        original_get(self)
Handler.do_GET = media_get

audio = io.BytesIO()
with wave.open(audio, 'wb') as stream:
    stream.setnchannels(2)
    stream.setsampwidth(2)
    stream.setframerate(48000)
    stream.writeframes(bytes(192000))

extra = r"""
bridge.idle=true;bridge.replayInactive=true;bridge.automaticUpdates=false;bridge.startupUpdateAvailable=false;
bridge.microphone='default';bridge.testPolls=0;bridge.microphoneGain=0;
bridge.volumeWrites=0;const originalVolumeSet=Storage.prototype.setItem;
Storage.prototype.setItem=function(key,value){if(key==='redunar.playback-volume.v1')bridge.volumeWrites++;return originalVolumeSet.call(this,key,value);};
const audioInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='set_microphone_gain'){
  bridge.calls.push({command,args});
  if(bridge.holdGain)await new Promise(resolve=>bridge.releaseGain=resolve);
  if(bridge.failGain)throw Error('Fixture gain save failed');
  bridge.microphoneGain=args.gainDb;return replayPreferencesFixture();
 }
 if(command==='start_microphone_test'){bridge.calls.push({command,args});bridge.testPolls=0;return {id:9,phase:'recording',seconds_recorded:0,levels:{peak:.6,rms:.3,limiting:false}};}
 if(command==='microphone_test_status'){
  bridge.testPolls++;
  if(bridge.keepRecording||bridge.testPolls===1)return {id:9,phase:'recording',seconds_recorded:3,levels:{peak:.88,rms:.4,limiting:true}};
  return {id:9,phase:'ready',seconds_recorded:6,levels:null};
 }
 if(command==='microphone_test_audio')return AUDIO_FIXTURE;
 if(command==='stop_microphone_test'){bridge.calls.push({command,args});return null;}
 if(command==='clip_playback_path')return '/__audio-controls-synthetic.mp4';
 if(command==='clip_metadata')return {duration_seconds:3,width:640,height:360,fps:10};
 return audioInvoke(command,args);
};
""".replace('AUDIO_FIXTURE', json.dumps(list(audio.getvalue())))
manager.add_script(WebKit2.UserScript.new(extra, WebKit2.UserContentInjectedFrames.TOP_FRAME,
                  WebKit2.UserScriptInjectionTime.START, None, None))

try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#global')
    wait_for("typeof q==='function'&&q('[data-global-tab=replay]')&&!q('[data-global-tab=replay]').disabled")
    js("click('[data-global-tab=replay]')")
    wait_for("q('#microphone-gain')&&!q('#microphone-gain').disabled")
    test('Idle never opens a microphone or polls a meter', "check(!bridge.calls.some(c=>c.command==='start_microphone_test'||c.command==='microphone_levels'),'no idle capture or metering')")
    test('Voice controls form one group beside a short explanation', "const fields=q('.microphone-fields'),slider=q('.microphone-gain-slider').getBoundingClientRect(),meter=q('.microphone-meter-wrap').getBoundingClientRect();check(fields.contains(q('#microphone-gain'))&&fields.contains(q('[data-microphone-test]')),'grouped controls');check(Math.abs(slider.left-meter.left)<1&&Math.abs(slider.right-meter.right)<1,'aligned slider and meter');check(Math.abs(q('#microphone-gain').getBoundingClientRect().width-slider.width)<1,'gain track fills its field');check(q('#microphone-gain').min==='-12'&&q('#microphone-gain').max==='24','original gain range');check(q('#microphone-gain-help').textContent.includes('Other apps stay unchanged'),'scope visible')")
    js("q('.microphone-controls').scrollIntoView({block:'center'})")
    snap('microphone-idle')
    js("const input=q('#microphone-gain');input.value=12;input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new Event('change',{bubbles:true}));")
    wait_for('bridge.microphoneGain===12&&!q("#microphone-gain").disabled')
    test('Recording gain is labelled and separately persisted', "check(q('.microphone-gain-slider output').textContent==='+12 dB','gain value');check(bridge.calls.some(c=>c.command==='set_microphone_gain'&&c.args.gainDb===12),'typed gain request')")
    js("bridge.holdGain=true;const input=q('#microphone-gain');input.value=18;input.dispatchEvent(new Event('change',{bubbles:true}));")
    wait_for('typeof bridge.releaseGain==="function"')
    route('overview')
    route('global')
    js("bridge.holdGain=false;bridge.releaseGain()")
    wait_for('q("#microphone-gain").value==="18"&&!q("#microphone-gain").disabled')
    test('A pending gain save stays consistent after returning to settings', "check(q('.microphone-gain-slider output').textContent==='+18 dB','late save updates recreated controls')")
    js("bridge.failGain=true;const input=q('#microphone-gain');input.value=24;input.dispatchEvent(new Event('change',{bubbles:true}));")
    wait_for('q(".microphone-test-status").dataset.error==="true"')
    test('Failed save restores the actual saved level', "check(q('#microphone-gain').value==='18','restored gain');bridge.failGain=false")
    js("click('[data-microphone-test]')")
    wait_for('q(".microphone-controls audio").src.startsWith("blob:")')
    wait_for('!q("[data-microphone-play]").disabled')
    test('Test playback is explicit and bounded local media', "const audio=q('.microphone-controls audio');check(audio.hidden&&audio.paused&&!q('.microphone-playback').hidden,'compact player without autoplay');check(!audio.controls,'no duplicate browser controls');check(q('.microphone-test-status').textContent.includes('Test ready'),'ready state')")
    js("click('[data-microphone-play]')")
    wait_for('!q(".microphone-controls audio").paused')
    test('Compact test controls play and pause only their own audio', "check(q('[data-microphone-play]').getAttribute('aria-label')==='Pause microphone test','pause label');click('[data-microphone-play]');check(q('.microphone-controls audio').paused,'pause works');check(q('[data-microphone-seek]').max==='1','actual test duration')")
    js("const seek=q('[data-microphone-seek]');seek.value=.5;seek.dispatchEvent(new Event('input',{bubbles:true}));")
    test('Test playback seeking leaves recording gain unchanged', "check(Math.abs(q('.microphone-controls audio').currentTime-.5)<.02,'seek works');check(bridge.microphoneGain===18&&q('#microphone-gain').value==='18','gain unchanged')")
    js("q('.microphone-controls').scrollIntoView({block:'center'})")
    pump(100)
    snap('microphone-test-ready')
    window.resize(600, 820)
    pump(200)
    js("q('.microphone-controls').scrollIntoView({block:'center'})")
    test('Grouped microphone controls fit a narrow settings panel', "const root=q('.microphone-controls'),bounds=root.getBoundingClientRect();check([...root.querySelectorAll('button,input,.microphone-meter,.microphone-playback')].filter(el=>el.getClientRects().length).every(el=>{const r=el.getBoundingClientRect();return r.left>=bounds.left&&r.right<=bounds.right+1}),'all controls fit');check(bridge.errors.length===0,JSON.stringify(bridge.errors))")
    snap('microphone-test-ready-narrow')
    window.resize(1440, 1000)
    pump(200)
    js("window.clearedAudio=q('.microphone-controls audio');click('[data-microphone-clear]')")
    test('Clearing test playback releases media and restores the idle controls', "check(window.clearedAudio.paused&&!window.clearedAudio.hasAttribute('src'),'media released');check(q('.microphone-playback').hidden,'player hidden');check(q('[data-microphone-test]').textContent==='Test microphone','test action restored')")
    js("bridge.keepRecording=true;click('[data-microphone-test]')")
    wait_for('q(".microphone-meter").getAttribute("aria-valuenow")==="88"')
    test('Real test levels and limiting are represented', "check(q('[data-microphone-peak]').textContent==='Peak protection','limiter indicator');check(q('#microphone-gain').disabled,'no mid-test gain change')")
    snap('microphone-test-recording')
    route('replay')
    wait_for('q("#clip-video")&&q("#clip-video").readyState>=2')
    test('Navigating away cancels only its named native test', "check(bridge.calls.some(c=>c.command==='stop_microphone_test'&&c.args.id===9),'test stopped')")
    js("const input=q('#clip-playback-volume');input.value=35;input.dispatchEvent(new Event('input',{bubbles:true}));")
    test('Playback volume changes listening only', "check(Math.abs(q('#clip-video').volume-.35)<.000001,'player volume');check(!bridge.calls.some(c=>c.command==='export_replay_clip'),'no export');check(q('#playback-volume-help').textContent.includes('exports stay unchanged'),'scope visible')")
    js("click('.playback-mute')")
    test('Mute and restore remember the listening level', "check(q('#clip-video').volume===0,'muted');click('.playback-mute');check(Math.abs(q('#clip-video').volume-.35)<.000001,'restored')")
    snap('playback-volume')
    js("window.oldPlayer=q('#clip-video');const other=qa('[data-clip]').find(el=>el.getAttribute('aria-pressed')!=='true');check(other,'another saved clip');other.click();")
    wait_for('q("#clip-video")!==window.oldPlayer&&q("#clip-video").readyState>=2')
    test('Slider reconnects after selecting another clip without reloading the page', "const input=q('#clip-playback-volume');input.value=18;input.dispatchEvent(new Event('input',{bubbles:true}));check(Math.abs(q('#clip-video').volume-.18)<.000001,'new slider changes new player');check(q('.playback-volume output').textContent==='18%','new output updates');click('.playback-mute');check(q('#clip-video').volume===0,'new mute works');click('.playback-mute');check(Math.abs(q('#clip-video').volume-.18)<.000001,'new unmute works')")
    pump(250)
    test('Drag bursts update immediately without per-input storage writes or speaker replacement', "const before=bridge.volumeWrites,speaker=q('.playback-mute .icon'),input=q('#clip-playback-volume');for(let value=1;value<=95;value++){input.value=value;input.dispatchEvent(new Event('input',{bubbles:true}));}check(bridge.volumeWrites===before,'no storage writes inside drag handlers');check(q('.playback-mute .icon')===speaker,'stable speaker icon while dragging');check(q('.playback-volume output').textContent==='95%','immediate label');check(Math.abs(q('#clip-video').volume-.95)<.000001,'immediate player volume');window.volumeBurstWrites=before;")
    pump(250)
    test('Only the final drag level is persisted', "check(bridge.volumeWrites===window.volumeBurstWrites+1,'one coalesced write');check(JSON.parse(localStorage.getItem('redunar.playback-volume.v1')).level===95,'latest level saved')")
    test('Clicking another control after dragging keeps it responsive', "click('.playback-mute');check(q('#clip-video').volume===0,'mute responds immediately');click('.playback-mute');check(Math.abs(q('#clip-video').volume-.95)<.000001,'unmute responds immediately');const input=q('#clip-playback-volume');input.value=35;input.dispatchEvent(new Event('input',{bubbles:true}));")

    route('overview')
    route('replay')
    wait_for('q("#clip-video")&&q("#clip-video").readyState>=2')
    test('Listening preference survives player recreation', "check(Math.abs(q('#clip-video').volume-.35)<.000001,'new player uses saved preference');check(bridge.errors.length===0,JSON.stringify(bridge.errors))")
    window.resize(960, 820)
    pump(400)
    test('Narrow player controls do not overlap', "const a=q('.playback-volume').getBoundingClientRect(),b=q('#clip-time').getBoundingClientRect();check(a.top>=b.bottom||a.left>=b.right,'volume and time do not overlap');check(a.right<=q('.editor').getBoundingClientRect().right,'volume fits inside editor');const children=[...q('.player-controls').children];check(children.every(el=>el.getBoundingClientRect().right<=q('.editor').getBoundingClientRect().right),'every control fits inside editor')")
    snap('playback-volume-narrow')
    js("click('.playback-mute')")
    view.reload()
    pump(300)
    wait_for('typeof q==="function"&&q("#clip-video")&&q("#clip-video").readyState>=2')
    test('Muted restart keeps the previous listening level for unmute', "check(q('#clip-video').volume===0,'restart stays muted');click('.playback-mute');check(Math.abs(q('#clip-video').volume-.35)<.000001,'unmute remembers quiet level')")
    (ARTIFACTS / 'checks.json').write_text(json.dumps(checks, indent=2)+'\n')
    print(f'PASS: {len(checks)} audio UI checks. {ARTIFACTS}')
finally:
    window.destroy()
    server.shutdown()
