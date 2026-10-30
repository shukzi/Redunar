"""Measure mounted player updates in WebKit after a frontend build.

Run with --baseline before the change, then without it after rebuilding.
Only synthetic media and the shared private native-command fixture are used.
"""
import argparse
import base64
import hashlib
import subprocess
from webview_fixture import *

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--baseline', action='store_true')
args = parser.parse_args()
REPORTS = NATIVE.parents[1] / '.redunar-build/tauri-optimization'
REPORTS.mkdir(parents=True, exist_ok=True)
media = []
for seconds in (12, 6):
    path = REPORTS / f'player-{seconds}.mp4'
    if not path.exists():
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f',
                        'lavfi', '-i', 'testsrc2=s=640x360:r=10', '-t', str(seconds),
                        '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-movflags',
                        '+faststart', str(path)], check=True)
    media.append('data:video/mp4;base64,' + base64.b64encode(path.read_bytes()).decode())

extra = r"""
bridge.idle=true;bridge.startupUpdateAvailable=false;
const originalInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='clip_playback_path'){
  bridge.calls.push({command,args});
  return MEDIA[args.fileName===bridge.clips[0].file_name?0:1];
 }
 return originalInvoke(command,args);
};
""".replace('MEDIA', json.dumps(media))
manager.add_script(WebKit2.UserScript.new(extra, WebKit2.UserContentInjectedFrames.TOP_FRAME,
                                        WebKit2.UserScriptInjectionTime.START, None, None))

try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#replay')
    wait_for("window.q&&q('#clip-video')?.readyState>=2&&!q('#player-seek').disabled", 15000)
    wait_for("q('#clip-filmstrip')?.dataset.frames==='8'", 15000)
    js("""
     window.video=q('#clip-video');window.rail=q('.clip-list');
     window.preparations=bridge.calls.filter(c=>c.command==='clip_playback_path').length;
     window.selected=q('.clip-card.selected [data-clip]').dataset.clip;
     q('#clip-search').value='Capture';q('#clip-search').dispatchEvent(new Event('input',{bubbles:true}));
     q('#clip-search').focus();q('#clip-search').setSelectionRange(1,4);
     window.search=q('#clip-search');
     window.playerSnapshot=()=>Object.fromEntries([
      'clip-time','clip-time-ruler','trim-start-value','trim-end-value','clip-selected-duration',
      'trim-start-bar','trim-end-bar','clip-playhead','trim-selection','clip-timeline','player-seek'
     ].map(id=>{const el=q('#'+id),r=el.getBoundingClientRect();return [id,{
      text:el.textContent,aria:Object.fromEntries([...el.attributes].filter(a=>a.name.startsWith('aria-')).map(a=>[a.name,a.value])),
      style:el.getAttribute('style'),disabled:el.disabled??null,value:el.value??null,
      bounds:[r.x,r.y,r.width,r.height]
     }];}));
     window.repeatUpdates=()=>{
      const before=playerSnapshot(),observer=new MutationObserver(()=>{});
      observer.observe(q('.replay-clip-editor'),{attributes:true,attributeOldValue:true,childList:true,characterData:true,subtree:true});
      observer.observe(q('.player-controls'),{attributes:true,attributeOldValue:true,childList:true,characterData:true,subtree:true});
      for(let i=0;i<100;i++)video.dispatchEvent(new Event('timeupdate'));
      const records=observer.takeRecords();observer.disconnect();
      const counts={};for(const r of records){const key=(r.target.id||r.target.nodeName)+':'+(r.attributeName||r.type);counts[key]=(counts[key]||0)+1;}
      const aria=records.filter(r=>r.type==='attributes'&&r.attributeName.startsWith('aria-'));
      const after=playerSnapshot();check(JSON.stringify(before)===JSON.stringify(after),'identical text, accessibility values and geometry');
      check(q('#clip-video')===video&&q('.clip-list')===rail,'mounted nodes retained');
      check(document.activeElement===search&&search.selectionStart===1&&search.selectionEnd===4,'focus and selection retained');
      check(q('.clip-card.selected [data-clip]').dataset.clip===selected,'clip selection retained');
      check(bridge.calls.filter(c=>c.command==='clip_playback_path').length===preparations,'no repeated preparation');
      return {updates:100,totalMutations:records.length,ariaMutations:aria.length,counts,before,after};
     };
    """)
    pump(250)
    initial = json.loads(js('return JSON.stringify(repeatUpdates())'))
    print(f'100 identical mounted updates: {initial["ariaMutations"]} ARIA / {initial["totalMutations"]} total mutations', flush=True)
    (REPORTS/('player-initial-before.json' if args.baseline else 'player-initial-after.json')).write_text(json.dumps(initial, indent=2)+'\n')
    expected = 1200 if args.baseline else 0
    assert initial['ariaMutations'] == expected, initial['counts']
    checks.append('100 identical media updates retain text, geometry, selection and focus')

    js("q('#player-seek').value='2.5';q('#player-seek').dispatchEvent(new Event('input',{bubbles:true}));")
    wait_for("Math.abs(video.currentTime-2.5)<.05&&q('#clip-timeline').getAttribute('aria-valuenow')==='2.50'")
    pump(200)
    test('Changed position updates text and playhead at the existing event cadence', """
     check(q('#clip-time').textContent==='00:02 / 00:12','timecode changed');
     check(q('#clip-timeline').getAttribute('aria-valuetext')==='00:02.5','accessible position changed');
     check(Math.abs(parseFloat(q('#clip-playhead').style.left)-100*2.5/12)<.001,'playhead moved');
     q('#trim-start-bar').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}));
    """)
    wait_for("q('#trim-start-bar').getAttribute('aria-valuenow')==='0.25'")
    pump(200)
    test('Keyboard trim updates dependent bounds, labels and selection', """
     check(q('#trim-start-value').textContent==='00:00.3','trim text changed');
     check(q('#trim-end-bar').getAttribute('aria-valuemin')==='0.35','dependent bound changed');
     check(q('#trim-start-bar').getAttribute('aria-valuetext')==='00:00.3','accessible trim changed');
     check(!q('#export-clip-button').hidden,'trim export available');
     search.focus();search.setSelectionRange(1,4);
    """)
    trimmed = json.loads(js('return JSON.stringify(repeatUpdates())'))
    assert trimmed['ariaMutations'] == expected, trimmed['counts']
    js("click('[data-action=reset-trim]')")
    wait_for("q('#trim-start-bar').getAttribute('aria-valuenow')==='0.00'&&q('#export-clip-button').hidden")
    js("click('#clip-play-toggle')")
    wait_for("!video.paused&&video.currentTime>.2&&Number(q('#clip-timeline').getAttribute('aria-valuenow'))>.2")
    test('Real decoded playback advances with accessible position updates', """
     check(q('#clip-play-toggle').getAttribute('aria-label')==='Pause clip','playing feedback');
     check(Number(q('#clip-timeline').getAttribute('aria-valuenow'))>0,'position advances');
     click('#clip-play-toggle');check(video.paused,'pause retained');
     window.oldVideo=video;click('[data-clip="'+bridge.clips[1].file_name+'"]');
    """)
    wait_for("q('#clip-video')!==oldVideo&&q('#clip-video').readyState>=2&&!q('#player-seek').disabled", 15000)
    test('Replacement player initializes new duration and old listeners are detached', """
     check(q('#clip-timeline').getAttribute('aria-valuemax')==='6','new duration');
     check(q('#trim-end-bar').getAttribute('aria-valuenow')==='6.00','new trim bound');
     check(q('#clip-time').textContent==='00:00 / 00:06','new text');
     const before=JSON.stringify(playerSnapshot());oldVideo.dispatchEvent(new Event('timeupdate'));
     check(JSON.stringify(playerSnapshot())===before,'detached player ignored');
     check(q('.clip-list')===rail,'rail retained');
    """)
    route('overview')
    test('Navigation releases playback DOM without fixture errors', """
     check(!q('#clip-video'),'player removed');check(oldVideo.paused,'old playback paused');
     check(bridge.errors.length===0,bridge.errors.join('; '));
    """)
    report = {'baseline': args.baseline, 'checks': checks, 'initial': initial, 'trimmed': trimmed,
              'webkit': '.'.join(str(v) for v in (WebKit2.get_major_version(), WebKit2.get_minor_version(), WebKit2.get_micro_version())),
              'build': {str(p.relative_to(NATIVE)): hashlib.sha256(p.read_bytes()).hexdigest()
                        for p in sorted((NATIVE/'ui/dist').rglob('*')) if p.is_file()}}
    baseline_path = REPORTS/'player-updates-before.json'
    if not args.baseline and baseline_path.exists():
        baseline = json.loads(baseline_path.read_text())
        for state in ('initial', 'trimmed'):
            assert baseline[state]['after'] == report[state]['after'], f'{state}: baseline text/geometry differs'
        report['baselineComparison'] = 'Identical final text, geometry, style and accessibility values'
    destination = baseline_path if args.baseline else REPORTS/'player-updates-after.json'
    destination.write_text(json.dumps(report, indent=2)+'\n')
    print(f'PASS: {len(checks)} player checks; {initial["ariaMutations"]} repeated ARIA mutations. Report: {destination}')
finally:
    window.destroy();server.shutdown()
