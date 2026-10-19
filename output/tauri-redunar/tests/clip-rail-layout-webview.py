"""Clip rail geometry in the built production UI with private native fixtures.

Run after rebuilding the production frontend:
  xvfb-run -a -s '-screen 0 2560x1600x24' python3 \
      output/tauri-redunar/tests/clip-rail-layout-webview.py
No native app, game, user clip, or installed helper is launched. Playback
preparation deliberately fails, leaving its normal recovery UI in place.
"""
import os
from pathlib import Path

os.environ.setdefault('GDK_BACKEND', 'x11')
os.environ['GDK_SCALE'] = '1'
os.environ['GDK_DPI_SCALE'] = '1'

if not (Path(__file__).resolve().parents[1] / 'ui/dist/index.html').is_file():
    raise SystemExit('Build the production frontend before running this fixture')

import webview_fixture as harness
from webview_fixture import *

ARTIFACTS = NATIVE.parents[1] / '.redunar-build/reports/clip-rail-layout'
ARTIFACTS.mkdir(parents=True, exist_ok=True)
harness.ARTIFACTS = ARTIFACTS

extra = r"""
bridge.idle=true;bridge.replayInactive=true;
bridge.automaticUpdates=false;bridge.startupUpdateAvailable=false;
// Stable filenames, timestamps and selection isolate inventory length from
// player geometry. All thumbnails and metadata remain the harness's fakes.
window.railClips=count=>Array.from({length:count},(_,i)=>({
 file_name:`redunar-replay-1792343672-${String(i+1).padStart(2,'0')}-small-small.mkv`,
 game_name:'Fixture game',bytes:7800000,modified_unix_ns:'1792343672000000000',
 duration_seconds:29,
}));
bridge.clips=railClips(2);
const railInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='clip_playback_path'){
  bridge.calls.push({command,args:structuredClone(args)});
  throw Error('Fixture playback preparation unavailable');
 }
 return railInvoke(command,args);
};
window.rail={
 near:(a,b,label)=>check(Math.abs(a-b)<=2,label+': '+a+' / '+b),
 box:element=>{const r=element.getBoundingClientRect();
  return {top:r.top,bottom:r.bottom,left:r.left,right:r.right,width:r.width,height:r.height};},
 inspect:()=>{
  const list=q('.clip-list');
  return {viewport:[innerWidth,innerHeight],browser:rail.box(q('.clip-browser')),
   editor:rail.box(q('.editor')),list:rail.box(list),storage:rail.box(q('.clip-storage')),
   client:list.clientHeight,extent:list.scrollHeight,scroll:list.scrollTop,
   pageExtent:q('.window-content').scrollHeight,
   cards:qa('.clip-card').map(card=>({card:rail.box(card),
    button:rail.box(card.querySelector('.clip-select')),
    thumbnail:rail.box(card.querySelector('.thumbnail')),
    title:rail.box(card.querySelector('.clip-title')),
    game:rail.box(card.querySelector('.clip-game')),
    details:rail.box(card.querySelector('small'))}))};
 },
 validate:desktop=>{
  const m=rail.inspect(),list=q('.clip-list');
  check(document.documentElement.scrollWidth<=innerWidth+1,'no horizontal page overflow');
  check(list.clientHeight>0&&list.clientWidth>0,'rail has usable area');
  check(list.scrollWidth<=list.clientWidth+1,'rail contents fit horizontally');
  check(m.storage.top>=m.list.bottom-2,'storage does not cover rail');
  check(m.storage.bottom<=m.browser.bottom+2,'storage remains inside browser');
  if(desktop){
   rail.near(m.editor.bottom,m.browser.bottom,'editor/browser bottoms aligned');
   rail.near(m.storage.top-m.list.bottom,parseFloat(getComputedStyle(list).marginBottom),'available space belongs to rail');
   check(m.list.height>=m.cards[0]?.card.height||m.cards.length===0,'at least one full card fits');
  }
  // Intentionally clamped filenames may span two lines, but a card must not
  // clip the displayed title/game/date/size inside its own selection button.
  for(const item of m.cards){
   check(item.title.height>0&&item.game.height>0&&item.details.height>0,'details have layout boxes');
   for(const box of [item.thumbnail,item.title,item.game,item.details]){
    check(box.top>=item.button.top-2&&box.bottom<=item.button.bottom+2,'card retains complete details');
    check(box.left>=item.button.left-2&&box.right<=item.button.right+2,'card contents fit button');
   }
   check(item.button.bottom<=item.card.bottom+2,'card does not shrink around its button');
  }
  return m;
 },
};
"""
manager.add_script(WebKit2.UserScript.new(
    extra, WebKit2.UserContentInjectedFrames.TOP_FRAME,
    WebKit2.UserScriptInjectionTime.START, None, None))

measurements = []


def inventory(count):
    """Refresh through the normal command, without changing UI-owned state."""
    js(f'bridge.failClips=false;bridge.clips=railClips({count});'
       'click("[data-action=reload-clips]");')
    wait_for(f'qa(".clip-card").length==={count}&&!document.body.classList.contains("native-busy")')
    pump(250)
    js('q(".clip-list").scrollTop=0;')


def record(label):
    result = json.loads(js('return JSON.stringify(rail.inspect());'))
    measurements.append({'case': label, **result})


def last_card(label):
    js('window.railPageTop=q(".window-content").scrollTop;'
       'window.railEditorTop=q(".editor").getBoundingClientRect().top;'
       'window.railSelection=q("[data-clip][aria-pressed=true]").dataset.clip;'
       'q(".clip-list").scrollTop=q(".clip-list").scrollHeight;')
    pump(150)
    test(label+' reaches the last complete clip without scrolling the page', r"""
        const list=q('.clip-list'),bounds=rail.box(list),last=qa('.clip-card').at(-1);
        const button=last.querySelector('.clip-select'),title=rail.box(button.querySelector('.clip-title'));
        const details=rail.box(button.querySelector('small'));
        check(title.top>=bounds.top-2&&details.bottom<=bounds.bottom+2,'last title/date/size reachable');
        rail.near(q('.window-content').scrollTop,railPageTop,'page stays stationary');
        rail.near(q('.editor').getBoundingClientRect().top,railEditorTop,'editor stays stationary');
        check(q('[data-clip][aria-pressed=true]').dataset.clip===railSelection,'scroll does not change selection');
    """)


try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#replay')
    wait_for('window.q&&qa(".clip-card").length===2&&document.fonts.status==="loaded"')
    pump(350)

    for width, height in [(1920, 800), (1238, 666), (1440, 1000), (640, 800)]:
        window.resize(width, height)
        pump(300)
        desktop = width > 980
        for count in (2, 3, 30):
            label = f'{width}x{height}, {count} clips'
            inventory(count)
            test(label+' retains readable cards and usable storage',
                 f'rail.validate({str(desktop).lower()});')
            if count == 2:
                js('window.railBaseline=rail.inspect();')
            elif desktop:
                test(label+' cannot grow the panels from the full inventory', r"""
                    const m=rail.inspect();
                    rail.near(m.editor.height,railBaseline.editor.height,'same editor height');
                    rail.near(m.browser.height,railBaseline.browser.height,'same browser height');
                    rail.near(m.client,railBaseline.client,'same available rail height');
                    rail.near(m.pageExtent,railBaseline.pageExtent,'inventory does not inflate page');
                """)
            if (width, height, count) == (1920, 800, 2):
                test('Two clips use the available desktop gap instead of hiding the second filename', r"""
                    const m=rail.inspect();
                    check(m.extent<=m.client+1,'two cards fit without unnecessary rail scrolling');
                    for(const item of m.cards)
                        check(item.thumbnail.top>=m.list.top-2&&item.details.bottom<=m.list.bottom+2,'both complete cards visible');
                """)
                snap('two-clips-1920x800')
            if count == 30:
                test(label+' keeps a long inventory in its native scrolling rail', r"""
                    const list=q('.clip-list');
                    check(list.scrollHeight>list.clientHeight,'long list overflows');
                    check(['auto','scroll'].includes(getComputedStyle(list).overflowY),'native vertical scrolling');
                """)
                last_card(label)
                snap(f'thirty-clips-{width}x{height}')
            record(label)

        # Refresh failure retains known clips in the same bounded rail; the
        # warning must not collapse the card space or cover storage/retry.
        inventory(3)
        js('window.railReads=bridge.calls.filter(c=>c.command==="replay_clips").length;'
           'bridge.failClips=true;click("[data-action=reload-clips]");')
        wait_for('bridge.calls.filter(c=>c.command==="replay_clips").length>railReads&&q(".clip-list [role=status]")&&!document.body.classList.contains("native-busy")')
        test(f'{width}x{height} failed refresh retains clips and recovery', f"""
            rail.validate({str(desktop).lower()});
            check(qa('.clip-card').length===3,'known clips retained');
            check(q('.clip-list [role=status]').textContent.includes('Fixture inventory unavailable'),'honest refresh failure');
            check(!q('[data-action=reload-clips]').disabled,'refresh remains available');
        """)
        last_card(f'{width}x{height} failed refresh')
        inventory(0)
        test(f'{width}x{height} empty inventory keeps guidance and storage usable', f"""
            rail.validate({str(desktop).lower()});
            check(q('.clip-empty')&&qa('.clip-card').length===0,'empty inventory guidance');
            check(!q('[data-action=reload-clips]').disabled,'refresh remains available');
        """)
        js('bridge.failClips=true;click("[data-action=reload-clips]");')
        wait_for('q(".clip-list [role=status]")&&!document.body.classList.contains("native-busy")')
        test(f'{width}x{height} empty error leaves visible failure and retry', f"""
            rail.validate({str(desktop).lower()});
            check(q('.clip-empty')&&q('.clip-list [role=status]'),'failure and empty guidance retained');
            check(!q('[data-action=reload-clips]').disabled,'failure can be retried');
        """)
        inventory(2)
        test(f'{width}x{height} recovery restores complete cards', f"""
            rail.validate({str(desktop).lower()});
            check(!q('.clip-list [role=status]'),'old failure cleared');
        """)

    test('Clip rail layout uses only private read fixtures', r"""
        check(bridge.errors.length===0,bridge.errors.join('; '));
        check(!bridge.calls.some(c=>['launch_game','end_session','save_replay','export_replay_clip','restart_redunar'].includes(c.command)),'no session/export side effects');
    """)
    (ARTIFACTS / 'clip-rail-layout-checks.json').write_text(json.dumps(checks, indent=2)+'\n')
    (ARTIFACTS / 'clip-rail-layout-measurements.json').write_text(json.dumps(measurements, indent=2)+'\n')
    print(f'PASS: {len(checks)} clip rail layout checks. Screenshots: {ARTIFACTS}')
except Exception:
    (ARTIFACTS / 'clip-rail-layout-partial-checks.json').write_text(json.dumps(checks, indent=2)+'\n')
    (ARTIFACTS / 'clip-rail-layout-measurements.json').write_text(json.dumps(measurements, indent=2)+'\n')
    snap('clip-rail-layout-failure')
    raise
finally:
    window.destroy()
    server.shutdown()
