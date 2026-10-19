"""Production UI polish with private commands, catalog, media and X input.

Run after the production frontend has been built:
  xvfb-run -a -s '-screen 0 1920x1200x24' env REDUNAR_PRIVATE_DESKTOP_FIXTURE=1 python3 \
      output/tauri-redunar/tests/ui-polish-webview.py
This never launches the native app, games, or installed helpers.
"""
import ctypes
import os
from pathlib import Path
import subprocess
import tempfile

if os.environ.get('REDUNAR_PRIVATE_DESKTOP_FIXTURE') != '1':
    raise SystemExit('Use xvfb-run with REDUNAR_PRIVATE_DESKTOP_FIXTURE=1 (private X input)')
if not (Path(__file__).resolve().parents[1] / 'ui/dist/index.html').is_file():
    raise SystemExit('Build the production frontend before running this fixture')
os.environ.setdefault('GDK_BACKEND', 'x11')
# Host desktop scaling must not change trusted X-input coordinates in Xvfb.
os.environ['GDK_SCALE'] = '1'
os.environ['GDK_DPI_SCALE'] = '1'

import webview_fixture as harness
from webview_fixture import *

gi.require_version('GdkX11', '3.0')
from gi.repository import GdkX11

ARTIFACTS = NATIVE.parents[1] / '.redunar-build/reports/ui-polish'
ARTIFACTS.mkdir(parents=True, exist_ok=True)
harness.ARTIFACTS = ARTIFACTS

extra = r"""
bridge.automaticUpdates=false;bridge.startupUpdateAvailable=false;
bridge.idle=true;bridge.replayInactive=true;
// Mutate the harness catalog in place so its save/reload closures see all rows.
games.splice(2,0,...Array.from({length:28},(_,i)=>({
 id:String(i+3),name:`Polish fixture ${String(i+3).padStart(2,'0')} with a long local title`,
 executable:`/fixture/ui-polish/game-${i+3}`,arguments:[],steam_app_id:null,
 revision:'1',launch_revision:`polish-${i+3}`,overrides:{}
})));
bridge.catalog=games;
bridge.installation=Object.fromEntries(games.map(game=>[game.id,'installed']));
bridge.clips=Array.from({length:3},(_,i)=>({file_name:`polish-fixture-${i}.mp4`,
 bytes:8000000,modified_unix_ns:String(1000000000+i),duration_seconds:3}));
const polishInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='session_status'&&bridge.failSession)throw Error('Fixture session status unavailable');
 if(command==='session_status'&&bridge.sessionStatus)return structuredClone(bridge.sessionStatus);
 if(command==='save_shortcuts'&&bridge.failShortcuts){
  bridge.calls.push({command,args:structuredClone(args)});
  throw Error('Fixture shortcut write rejected');
 }
 if(command==='save_game_profile'&&bridge.failGameProfile){
  bridge.calls.push({command,args:structuredClone(args)});
  throw Error('Fixture profile write rejected');
 }
 if(command==='clip_playback_path'){
  bridge.calls.push({command,args:structuredClone(args)});
  if(bridge.failPlayback)throw Error('Fixture playback preparation rejected');
  return '/__ui-polish-media.mp4?clip='+encodeURIComponent(args.fileName);
 }
 if(command==='clip_metadata')return {duration_seconds:3,width:320,height:180,fps:10};
 if(command==='clip_export_status')return {active:false,progress:0};
 // Export submission is deliberately unavailable; opening/cancelling the form
 // must never escape into a permissive fake-success response.
 if(command==='export_replay_clip'){
  bridge.calls.push({command,args:structuredClone(args)});
  throw Error('Fixture export submission rejected');
 }
 return polishInvoke(command,args);
};
window.polish={
 inputEvents:[],
 writes:()=>bridge.calls.filter(c=>['save_global_settings','save_shortcuts','save_game_profile'].includes(c.command)),
 set:(selector,value,type='input')=>{const input=q(selector);check(input,'missing '+selector);
  input.value=value;input.dispatchEvent(new Event(type,{bubbles:true}));},
 visible:element=>!!element&&!element.hidden&&getComputedStyle(element).visibility!=='hidden'
  &&getComputedStyle(element).display!=='none'&&element.getBoundingClientRect().width>0,
 near:(a,b,label)=>check(Math.abs(a-b)<=2,label+': '+a+' / '+b),
 counts:()=>{
  const groups=qa('.metric-group');check(groups.length===2,'frame and hardware groups');
  for(const group of groups){
   const inputs=[...group.querySelectorAll('[data-metric]')];
   const numbers=group.querySelector('.metric-group-count').textContent.match(/\d+/g)?.map(Number);
   check(inputs.length>0&&numbers?.length===2,'group has metrics and readable counts');
   check(numbers[0]===inputs.filter(i=>i.checked).length&&numbers[1]===inputs.length,'group count matches controls');
  }
  check(parseInt(q('.overlay-metric-count').textContent,10)===qa('[data-metric]:checked').length,'total count matches controls');
 },
 panel:key=>{
  for(const panel of qa('[data-overlay-editor-panel]')){
   const active=panel.dataset.overlayEditorPanel===key;
   check(panel.inert===!active,'inactive panel is inert');
   check(panel.hasAttribute('data-editor-inactive')===!active,'inactive attribute');
   check(panel.getAttribute('aria-hidden')===String(!active),'accessible panel visibility');
   const tab=q('[data-overlay-editor-tab='+panel.dataset.overlayEditorPanel+']');
   check(tab.getAttribute('aria-selected')===String(active),'selected inner tab');
   check(tab.tabIndex===(active?0:-1),'one tab in keyboard order');
   check(polish.visible(panel)===active,'only active panel paints');
  }
 },
 fields:root=>{
  const rows=[...root.querySelectorAll('.field')].filter(polish.visible);
  check(rows.length>0,'visible field rows');
  for(const row of rows){const s=getComputedStyle(row);
   check(s.borderBottomStyle==='none'||parseFloat(s.borderBottomWidth)===0||s.borderBottomColor==='rgba(0, 0, 0, 0)','no field underline');}
 },
 fit:()=>check(document.documentElement.scrollWidth<=innerWidth+1,'no horizontal page overflow'),
};
for(const type of ['pointerdown','click'])document.addEventListener(type,event=>{
 if(event.isTrusted)polish.inputEvents.push({type,x:event.clientX,y:event.clientY,
  target:event.target.outerHTML.slice(0,300)});
},true);
"""
manager.add_script(WebKit2.UserScript.new(
    extra, WebKit2.UserContentInjectedFrames.TOP_FRAME,
    WebKit2.UserScriptInjectionTime.START, None, None))

# Borrow the existing loopback server; media stays in a private temporary folder,
# never in ui/dist or the user's recordings. No native playback command runs.
original_get = harness.Handler.do_GET
media_body = b''


def serve_media(handler):
    if handler.path.split('?')[0] != '/__ui-polish-media.mp4':
        return original_get(handler)
    handler.send_response(200)
    handler.send_header('Content-Type', 'video/mp4')
    handler.send_header('Content-Length', str(len(media_body)))
    handler.end_headers()
    try:
        handler.wfile.write(media_body)
    except (BrokenPipeError, ConnectionResetError):
        pass  # Selection/cancellation can close an obsolete media request.


harness.Handler.do_GET = serve_media
display = None
x11 = None


def native_key(name):
    symbol = x11.XStringToKeysym(name.encode('ascii'))
    code = x11.XKeysymToKeycode(display, symbol)
    if not code:
        raise RuntimeError('Private X display has no keycode for ' + name)
    for pressed in (1, 0):
        if not xtest.XTestFakeKeyEvent(display, code, pressed, 0):
            raise RuntimeError('Private key injection failed')
    x11.XFlush(display)
    pump(100)


def native_motion(selector):
    js('q('+json.dumps(selector)+').scrollIntoView({block:"center"})')
    pump(150)
    point = json.loads(js('const r=q('+json.dumps(selector)+').getBoundingClientRect();'
                         'check(r.width>0&&r.height>0,"pointer target visible");'
                         'return JSON.stringify([r.left+r.width/2,r.top+r.height/2]);'))
    # WebKit is a windowless GTK child: its parent Gdk.Window origin omits
    # GTK's client-side titlebar/allocation offset. Translate through Gtk.Window
    # before injecting root-screen coordinates, as a real pointer would use.
    origin = window.get_window().get_origin()
    offset = view.translate_coordinates(window, 0, 0)
    if offset is None:
        raise RuntimeError('Cannot translate private webview coordinates')
    if not xtest.XTestFakeMotionEvent(display, -1, round(origin[-2]+offset[-2]+point[0]),
                                      round(origin[-1]+offset[-1]+point[1]), 0):
        raise RuntimeError('Private pointer motion failed')
    x11.XFlush(display)
    pump(150)


def native_click(selector):
    native_motion(selector)
    for pressed in (1, 0):
        if not xtest.XTestFakeButtonEvent(display, 1, pressed, 0):
            raise RuntimeError('Private pointer click failed')
    x11.XFlush(display)
    pump(200)


try:
    x11 = ctypes.CDLL('libX11.so.6')
    xtest = ctypes.CDLL('libXtst.so.6')
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
    x11.XStringToKeysym.restype = ctypes.c_ulong
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_uint
    x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    display = x11.XOpenDisplay(None)
    if not display:
        raise RuntimeError('Private X display unavailable')
    window.present()
    x11.XSetInputFocus(display, GdkX11.X11Window.get_xid(window.get_window()), 1, 0)

    with tempfile.TemporaryDirectory(prefix='redunar-ui-polish-') as temporary:
        movie = Path(temporary) / 'fixture.mp4'
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi',
                        '-i', 'testsrc2=s=320x180:r=10', '-t', '3', '-an', '-c:v',
                        'libx264', '-pix_fmt', 'yuv420p', '-movflags', '+faststart',
                        str(movie)], check=True, timeout=30)
        media_body = movie.read_bytes()

    view.load_uri(f'http://127.0.0.1:{server.server_port}/#global')
    wait_for("window.q&&q('.overlay-settings-editor')&&q('.live-overlay-panel')&&q('[data-global=scale]')")
    pump(500)
    test('Clean overlay has two mounted accessible editor views', r"""
        check(qa('[data-overlay-editor-tab]').length===2,'two inner tabs');polish.panel('appearance');
        check(q('.settings-save-bar').hidden,'no initial draft');polish.counts();
        window.polishBaseline=structuredClone(values);window.polishWrites=polish.writes().length;
        const inactive=q('[data-overlay-editor-panel=metrics] input');inactive.focus();
        check(document.activeElement!==inactive,'inert controls cannot take focus');
        polish.fields(q('.overlay-settings-editor'));
    """)
    test('Live overlay and session metrics are stacked equal cards aligned to editor', r"""
        const cards=qa('.overlay-preview-features>.panel');check(cards.length===2,'two independent feature cards');
        const a=cards[0].getBoundingClientRect(),b=cards[1].getBoundingClientRect();
        polish.near(a.left,b.left,'stacked left');polish.near(a.right,b.right,'equal width');
        polish.near(a.height,b.height,'equal height');check(b.top>=a.bottom,'cards stack without overlap');
        polish.near(b.bottom,q('.overlay-settings-editor').getBoundingClientRect().bottom,'shared bottom');
    """)
    test('Idle overlay visibility stays below preview and requires Save', r"""
        const input=q('[data-global=overlay]');
        check(input.closest('.live-overlay-panel'),'idle visibility stays in its own left card');
        check(!q('.overlay-appearance-fields [data-global=overlay]'),'no duplicated appearance control');
        check(q('.overlay-save-mode').textContent==='Saved with settings','idle save semantics explicit');
        check(q('.live-overlay-panel .field p').textContent.includes('future game sessions'),'idle explanation');
        window.polishVisibilityInput=input;window.polishCards=qa('.overlay-preview-features>.panel');
        window.polishIdleWrites=polish.writes().length;input.click();
        polish.set('[data-global=scale]','105');input.focus();
        check(polish.writes().length===polishIdleWrites,'idle change does not auto-save');
        check(values.overlay&&!input.checked,'idle visibility draft separate from saved default');
        check(q('#global-save-feedback').textContent.includes('Overlay visibility'),'visibility draft named');
    """)
    pump(200)
    snap('overlay-idle-draft-wide')
    js('bridge.idle=false;bridge.replayInactive=false;')
    wait_for("q('.overlay-save-mode').textContent==='Auto-saved'")
    test('Session start updates the explanation without moving controls or dropping drafts', r"""
        check(q('[data-global=overlay]')===polishVisibilityInput,'visibility node retained');
        check(polishCards.every((card,i)=>card===qa('.overlay-preview-features>.panel')[i]),'feature cards retained');
        check(document.activeElement===polishVisibilityInput,'focus retained');
        check(!polishVisibilityInput.checked&&q('[data-global=scale]').value==='105','idle drafts retained');
        check(q('.live-overlay-panel .field p').textContent.includes('immediately'),'live explanation');
        polishVisibilityInput.click();
    """)
    wait_for('polish.writes().length>polishIdleWrites&&!q("[data-global=overlay]").disabled')
    test('Live visibility saves only visibility and retains pending appearance', r"""
        const write=polish.writes().at(-1);check(write.command==='save_global_settings','real save command');
        check(write.args.input.overlay&&write.args.input.scale===polishBaseline.scale,'visibility-only payload');
        check(q('[data-global=scale]').value==='105'&&values.scale===polishBaseline.scale,'appearance draft retained');
        check(!q('.settings-save-bar').hidden,'appearance still needs Save');
        bridge.conflict=true;window.polishLiveWrites=polish.writes().length;
        q('[data-global=overlay]').click();
    """)
    wait_for('polish.writes().length>polishLiveWrites&&!q("[data-global=overlay]").disabled')
    test('Rejected live visibility restores saved visibility without losing appearance', r"""
        check(q('[data-global=overlay]').checked&&values.overlay,'failed immediate save rolled back');
        check(q('[data-global=scale]').value==='105','appearance survived failure');
        bridge.conflict=false;bridge.idle=true;bridge.replayInactive=true;
    """)
    wait_for("q('.overlay-save-mode').textContent==='Saved with settings'")
    test('Session end restores explicit Save semantics in the same card', r"""
        check(q('[data-global=overlay]')===polishVisibilityInput,'same input after session ends');
        window.polishEndedWrites=polish.writes().length;q('[data-global=overlay]').click();
        check(polish.writes().length===polishEndedWrites,'idle change again waits for Save');
        click('[data-action=discard-global]');
        check(q('[data-global=overlay]').checked&&q('[data-global=scale]').value===String(polishBaseline.scale),'Discard restores both drafts');
        bridge.idle=false;bridge.replayInactive=false;
        window.polishWrites=polish.writes().length;
    """)
    wait_for("q('.overlay-save-mode').textContent==='Auto-saved'")
    js("window.polishVisibilityInput=q('[data-global=overlay]');bridge.modules.in_game_overlay='Unavailable';bridge.sessionStatus={phase:'CleanupPending',can_end:true,launch_locked:true};")
    pump(2800)
    test('An owned session with unavailable capture promises saving, not live readiness', r"""
        check(q('.overlay-save-mode').textContent==='Auto-saved','automatic persistence label');
        const text=q('.live-overlay-panel').textContent;
        check(text.includes('when available')&&!text.includes('Applies immediately'),'no false overlay readiness');
        check(q('[data-global=overlay]')===polishVisibilityInput,'same control in cleanup state');
        bridge.failSession=true;
    """)
    wait_for("q('.overlay-save-mode').textContent==='Saved with settings'")
    test('Unknown session status retains cards and drafts without claiming live behavior', r"""
        check(qa('.overlay-preview-features>.panel').length===2,'both cards still mounted');
        check(q('[data-global=overlay]')===polishVisibilityInput,'same control on rejected read');
        check(q('.settings-save-bar').hidden,'poll failure does not create a draft');
        bridge.failSession=false;bridge.sessionStatus=null;bridge.modules.in_game_overlay='Enabled';
    """)
    wait_for("q('.overlay-save-mode').textContent==='Auto-saved'")
    snap('overlay-appearance-wide')
    test('Appearance and custom metric drafts survive both inner tabs', r"""
        polish.set('[data-global=scale]','110');click('button[data-layout=Ribbon]');
        q('[data-global=branding]').click();click('[data-overlay-editor-tab=metrics]');polish.panel('metrics');
        click('[data-preset=Custom]');
        const metric=qa('[data-metric]').find(i=>!i.checked);check(metric,'unselected custom metric');
        window.polishMetric=metric.dataset.metric;metric.click();polish.counts();
        click('[data-overlay-editor-tab=appearance]');polish.panel('appearance');
        check(q('[data-global=scale]').value==='110','scale draft retained');
        check(q('button[data-layout=Ribbon]').getAttribute('aria-pressed')==='true','layout retained');
        check(!q('[data-global=branding]').checked,'branding draft retained');
        click('[data-overlay-editor-tab=metrics]');polish.panel('metrics');
        check(qa('[data-metric]').find(i=>i.dataset.metric===polishMetric).checked,'metric draft retained');
        check(polish.writes().length===polishWrites,'tabs and edits do not save');
        const text=q('#global-save-feedback').textContent;
        check(text.includes('Overlay appearance')&&text.includes('Displayed metrics'),'all pending scopes named');
    """)
    test('Metric groups keep checked and unchecked rows plain', r"""
        for(const label of qa('.metric-group .metric-toggles label')){
            const s=getComputedStyle(label);
            check(s.backgroundImage==='none'&&s.backgroundColor==='rgba(0, 0, 0, 0)','no metric card fill');
            check(s.boxShadow==='none','no row shadow');
            for(const edge of ['Top','Right','Bottom','Left'])
                check(parseFloat(s['border'+edge+'Width'])===0||s['border'+edge+'Color']==='rgba(0, 0, 0, 0)','no metric card border');
        }
    """)
    snap('overlay-metrics-wide')
    test('An empty Custom selection blocks saving and recovers after one metric', r"""
        for(const input of qa('[data-metric]:checked'))input.click();polish.counts();
        check(q('[data-action=save-global]').disabled,'empty Custom cannot save');
        check(q('#metric-validation').textContent.trim().length>0,'validation explains failure');
        q('[data-metric=FPS]').click();polish.counts();
        check(!q('[data-action=save-global]').disabled,'valid selection can save');
    """)
    test('FPS only disables layout and branding without overwriting their draft values', r"""
        click('[data-preset="FPS only"]');polish.counts();
        check(qa('[data-metric]:checked').length===1&&q('[data-metric=FPS]').checked,'only FPS shown');
        click('[data-overlay-editor-tab=appearance]');
        check(qa('button[data-layout]').every(b=>b.disabled),'all layouts disabled');
        const branding=q('[data-global=branding]');check(branding.disabled&&!branding.checked,'branding retained but unavailable');
        check(q('button[data-layout=Ribbon]').getAttribute('aria-pressed')==='true','retained layout selected');
        q('button[data-layout=Grid]').click();branding.click();
        check(q('button[data-layout=Ribbon]').getAttribute('aria-pressed')==='true'&&!branding.checked,'disabled clicks change nothing');
        click('[data-overlay-editor-tab=metrics]');click('[data-preset=Detailed]');
        click('[data-overlay-editor-tab=appearance]');
        check(qa('button[data-layout]').every(b=>!b.disabled),'layouts restored');
        check(!q('[data-global=branding]').disabled&&!q('[data-global=branding]').checked,'branding restored without resetting value');
        check(q('#hud').dataset.layout==='Ribbon','preview restores retained layout');
        click('[data-action=discard-global]');
        check(q('[data-global=scale]').value===String(polishBaseline.scale),'Discard restores scale');
        check(q('[data-global=branding]').checked===polishBaseline.branding,'Discard restores branding');
        check(q('button[data-layout="'+polishBaseline.layout+'"]').getAttribute('aria-pressed')==='true','Discard restores saved layout');
        check(q('.settings-save-bar').hidden&&polish.writes().length===polishWrites,'Discard commits nothing');
    """)
    test('FPS-only Save keeps the previous layout and branding in the native payload', r"""
        click('button[data-layout=Ribbon]');q('[data-global=branding]').click();
        click('[data-overlay-editor-tab=metrics]');click('[data-preset="FPS only"]');
        click('[data-action=save-global]');
    """)
    wait_for("q('.settings-save-bar').hidden&&values.preset==='FPS only'")
    route('overview');route('global')
    test('Remounting saved FPS-only settings restores controls without resetting their values', r"""
        click('[data-overlay-editor-tab=appearance]');
        check(values.layout==='Ribbon'&&values.branding===false,'saved values retained by native response');
        check(qa('button[data-layout]').every(b=>b.disabled),'saved FPS-only layouts remain disabled');
        check(q('[data-global=branding]').disabled&&!q('[data-global=branding]').checked,'saved branding retained');
        click('[data-overlay-editor-tab=metrics]');click('[data-preset=Detailed]');
        click('[data-overlay-editor-tab=appearance]');
        check(q('button[data-layout=Ribbon]').getAttribute('aria-pressed')==='true'&&!q('button[data-layout=Ribbon]').disabled,'previous layout restored');
        check(!q('[data-global=branding]').disabled&&!q('[data-global=branding]').checked,'previous branding restored');
        click('button[data-layout="'+polishBaseline.layout+'"]');
        if(q('[data-global=branding]').checked!==polishBaseline.branding)q('[data-global=branding]').click();
        click('[data-overlay-editor-tab=metrics]');click('[data-preset="'+polishBaseline.preset+'"]');
        click('[data-action=save-global]');
    """)
    wait_for("q('.settings-save-bar').hidden&&values.preset===polishBaseline.preset")
    js("click('[data-overlay-editor-tab=appearance]');window.polishWrites=polish.writes().length;")
    test('Rejected global save retains drafts and actionable scope feedback', r"""
        polish.set('[data-global=scale]','105');bridge.conflict=true;
        window.polishFailedWrites=polish.writes().length;click('[data-action=save-global]');
    """)
    wait_for("polish.writes().length>polishFailedWrites&&!q('[data-action=save-global]').disabled&&!q('#toast').hidden")
    test('Global failure leaves the saved baseline intact', r"""
        check(values.scale===polishBaseline.scale,'rejected save did not commit');
        check(q('[data-global=scale]').value==='105'&&!q('.settings-save-bar').hidden,'draft retained');
        check(q('#global-save-feedback').textContent.includes('Overlay appearance'),'scope retained');
        bridge.conflict=false;click('[data-action=discard-global]');
    """)

    # Trusted X input exercises WebKit's :focus-visible heuristic; setting the
    # data-input attribute or dispatching a JS click would hide this regression.
    js("q('[data-overlay-editor-tab=appearance]').focus();")
    view.grab_focus()
    native_key('End')
    wait_for("q('[data-overlay-editor-tab=metrics]').getAttribute('aria-selected')==='true'")
    pump(250)
    test('Keyboard navigation retains accessible focus', r"""
        polish.panel('metrics');const tab=q('[data-overlay-editor-tab=metrics]');
        check(document.activeElement===tab,'arrow navigation moves focus');
        const underline=q('.overlay-editor-tabs>.selection-indicator').getBoundingClientRect(),box=tab.getBoundingClientRect();
        polish.near(underline.left,box.left,'keyboard underline follows active tab');
        polish.near(underline.width,box.width,'keyboard underline matches active tab');
        check(document.documentElement.dataset.input==='keyboard','trusted key selects keyboard modality');
        const s=getComputedStyle(tab);check(s.outlineStyle!=='none'&&parseFloat(s.outlineWidth)>0,'keyboard focus remains visible');
    """)
    native_motion('[data-overlay-editor-tab=appearance]')
    test('Passive pointer movement preserves keyboard modality and focus visibility', r"""
        const tab=q('[data-overlay-editor-tab=metrics]'),s=getComputedStyle(tab);
        check(document.documentElement.dataset.input==='keyboard','movement does not select pointer modality');
        check(document.activeElement===tab,'movement leaves keyboard focus in place');
        check(s.outlineStyle!=='none'&&parseFloat(s.outlineWidth)>0,'movement keeps keyboard focus visible');
    """)
    native_click('[data-overlay-editor-tab=appearance]')
    test('Pointer use after keyboard clears outlines without changing a draft', r"""
        check(document.documentElement.dataset.input==='pointer','root input modality is pointer');
        polish.panel('appearance');const tab=q('[data-overlay-editor-tab=appearance]'),s=getComputedStyle(tab);
        check(document.activeElement===tab,'real pointer focused clicked control');
        check(s.outlineStyle==='none'||parseFloat(s.outlineWidth)===0,'no stale keyboard outline');
        check(q('.settings-save-bar').hidden,'inner navigation stays clean');polish.fields(q('.overlay-settings-editor'));
    """)
    js("q('[data-global=scale]').focus();")
    native_key('Right')
    pump(250)
    test('Keyboard edits resize the selected global-tab underline with its dirty marker', r"""
        const tab=q('.global-tabs button.active'),line=q('.global-tabs>.selection-indicator');
        check(!tab.querySelector('.unsaved-tab-marker').hidden,'keyboard edit reveals marker');
        polish.near(line.getBoundingClientRect().width,tab.getBoundingClientRect().width,'dirty underline width');
    """)
    native_key('Left')
    pump(250)
    test('Keyboard restoration removes the marker and restores underline geometry', r"""
        const tab=q('.global-tabs button.active'),line=q('.global-tabs>.selection-indicator');
        check(tab.querySelector('.unsaved-tab-marker').hidden,'restored value clears marker');
        polish.near(line.getBoundingClientRect().width,tab.getBoundingClientRect().width,'clean underline width');
    """)
    native_click('[data-global=scale]')
    test('A clicked slider also clears the keyboard outline without a field underline', r"""
        const slider=q('[data-global=scale]'),s=getComputedStyle(slider);
        check(document.activeElement===slider,'real pointer focused slider');
        check(document.documentElement.dataset.input==='pointer','slider click selects pointer modality');
        check(s.outlineStyle==='none'||parseFloat(s.outlineWidth)===0,'clicked slider has no outline');
        polish.fields(q('.overlay-settings-editor'));click('[data-action=discard-global]');
        check(q('.settings-save-bar').hidden,'slider draft can be discarded');
    """)

    js("q('[data-global=captureMetrics]').focus();")
    native_key('space')
    test('Keyboard focus still identifies a session-metrics switch', r"""
        const input=q('[data-global=captureMetrics]');
        check(document.activeElement===input&&document.documentElement.dataset.input==='keyboard','trusted keyboard focus');
        check([input,input.nextElementSibling,input.closest('.switch')].some(el=>{const s=getComputedStyle(el);return s.outlineStyle!=='none'&&parseFloat(s.outlineWidth)>0;}),'original switch keyboard cue');
    """)
    native_click('[data-global=captureMetrics]')
    test('A clicked switch clears input, track and ancestor outlines', r"""
        const input=q('[data-global=captureMetrics]');
        check(document.documentElement.dataset.input==='pointer','trusted switch click selects pointer mode');
        for(const el of [input,input.nextElementSibling,input.closest('.switch')]){
         const s=getComputedStyle(el);check(s.outlineStyle==='none'||parseFloat(s.outlineWidth)===0,'no lingering switch ring');
        }
        check(input.checked===polishBaseline.captureMetrics&&q('.settings-save-bar').hidden,'two toggles restore the clean draft');
    """)

    test('Shortcut tab describes separate pending save owners', r"""
        polish.set('[data-global=scale]','105');click('[data-global-tab=replay]');
        polish.set('[data-global=fps]','240','change');click('[data-global-tab=shortcuts]');
        const input=q('[data-shortcut]');input.focus();
        input.dispatchEvent(new KeyboardEvent('keydown',{key:'F8',ctrlKey:true,bubbles:true,cancelable:true}));
        const text=q('#global-save-feedback').textContent;
        for(const label of ['Keyboard shortcuts','Overlay appearance','Replay settings'])check(text.includes(label),'pending scope '+label);
        check(/separately/i.test(text),'profile pending separately from shortcut owner');
        check(/shortcut/i.test(q('[data-action=save-global]').textContent),'save identifies shortcut owner');
        window.polishShortcutDraft=input.value;window.polishProfileWrites=bridge.calls.filter(c=>c.command==='save_global_settings').length;
        bridge.failShortcuts=true;click('[data-action=save-global]');
    """)
    wait_for("bridge.calls.some(c=>c.command==='save_shortcuts')&&!q('[data-action=save-global]').disabled&&!q('#toast').hidden")
    test('Failed shortcut save preserves both owners and supports retry', r"""
        check(q('[data-shortcut]').value===polishShortcutDraft,'failed shortcut draft retained');
        check(q('#global-save-feedback').textContent.includes('Keyboard shortcuts'),'failed scope retained');
        check(bridge.calls.filter(c=>c.command==='save_global_settings').length===polishProfileWrites,'shortcut failure does not save profile');
        bridge.failShortcuts=false;click('[data-action=save-global]');
    """)
    wait_for("values.shortcuts.overlay===polishShortcutDraft&&q('[data-action=save-global]').disabled")
    test('Successful shortcut save leaves appearance and replay drafts pending', r"""
        check(!q('.settings-save-bar').hidden,'other owner keeps bar visible');
        check(bridge.calls.filter(c=>c.command==='save_global_settings').length===polishProfileWrites,'only shortcuts saved');
        check(values.scale===polishBaseline.scale&&values.fps===polishBaseline.fps,'profile baseline unchanged');
        click('[data-global-tab=overlay]');click('[data-overlay-editor-tab=appearance]');
        check(q('[data-global=scale]').value==='105','appearance retained after shortcut save');
        click('[data-global-tab=replay]');check(q('[data-global=fps]').value==='240','replay retained');
        click('[data-action=discard-global]');check(q('.settings-save-bar').hidden,'Discard clears remaining owner');
    """)

    route('library')
    wait_for("qa('#library-list [data-game]').length===30&&q('.game-detail')")
    pump(400)
    test('Thirty fixture games scroll independently within the detail height', r"""
        const list=q('#library-list'),detail=q('.game-detail'),r=detail.getBoundingClientRect();
        check(list.scrollHeight>list.clientHeight&&list.clientHeight>0,'catalog really overflows');
        check(['auto','scroll'].includes(getComputedStyle(list).overflowY),'catalog owns vertical scroll');
        check(list.getBoundingClientRect().bottom<=r.bottom+2,'list bounded by detail');
        check(q('.game-catalog').getBoundingClientRect().bottom<=r.bottom+2,'catalog does not inflate detail');
        window.polishOuterScroll=q('.window-content').scrollTop;window.polishDetailTop=r.top;
        list.scrollTop=list.scrollHeight;
    """)
    pump(150)
    test('Catalog scrolling leaves the detail and page stationary', r"""
        check(q('#library-list').scrollTop>0,'catalog moved');
        polish.near(q('.window-content').scrollTop,polishOuterScroll,'page scroll unchanged');
        polish.near(q('.game-detail').getBoundingClientRect().top,polishDetailTop,'detail stationary');
        const list=q('#library-list').getBoundingClientRect(),last=q('[data-game="30"]').getBoundingClientRect();
        check(last.top>=list.top-2&&last.bottom<=list.bottom+2,'last game reachable inside list');
        q('#library-list').scrollTop=0;
    """)
    test('Default local rows are quiet and saved custom rows stay identifiable', r"""
        const row=q('[data-game="3"]'),status=row.querySelector('[data-profile-status]');
        check(status.hidden&&!status.textContent.trim(),'no default-profile badge');
        check([...row.querySelectorAll('small')].filter(polish.visible).length===0,'no repeated local launcher label');
        const custom=q('[data-game="2"] [data-profile-status]');
        check(polish.visible(custom)&&/custom/i.test(custom.textContent),'saved custom profile is visible');
        click('[data-game="3"]');polish.set('[data-override=overlay]','Off','change');
        const pending=q('[data-game="3"] [data-profile-status]');
        check(polish.visible(pending)&&/unsaved|draft/i.test(pending.textContent),'draft status visible');
        bridge.failGameProfile=true;click('[data-action=save-game]');
    """)
    wait_for("bridge.calls.some(c=>c.command==='save_game_profile')&&!q('[data-action=save-game]').disabled&&!q('#toast').hidden")
    test('Failed game save preserves the draft indicator and retry', r"""
        check(q('[data-override=overlay]').value==='Off'&&!q('.game-save-bar').hidden,'failed game draft retained');
        check(/unsaved|draft/i.test(q('[data-game="3"] [data-profile-status]').textContent),'failure remains marked');
        bridge.failGameProfile=false;click('[data-action=save-game]');
    """)
    wait_for("q('.game-save-bar').hidden")
    test('Saved custom indicator survives Discard and does not spread to default games', r"""
        const status=q('[data-game="3"] [data-profile-status]');
        check(polish.visible(status)&&/custom/i.test(status.textContent)&&!/unsaved|draft/i.test(status.textContent),'saved custom without draft');
        polish.set('[data-override=overlay]','On','change');click('[data-action=discard-game]');
        check(q('[data-override=overlay]').value==='Off','Discard returns to saved override');
        check(q('[data-game="4"] [data-profile-status]').hidden,'unrelated default row remains quiet');
        polish.set('#game-search','no-such-polish-fixture');check(qa('#library-list [data-game]').length===0,'empty search is honest');
        polish.set('#game-search','');check(qa('#library-list [data-game]').length===30,'clearing search restores all fixtures');
    """)
    snap('library-many-games-wide')
    window.resize(640, 800);pump(400)
    test('Narrow Library retains usable independent browsing', r"""
        polish.fit();check(q('#library-list').scrollHeight>q('#library-list').clientHeight,'narrow list bounded');
        check(q('.game-detail').getBoundingClientRect().right<=innerWidth+1,'detail fits');
        check(q('#library-list').scrollWidth<=q('#library-list').clientWidth+1,'long catalog titles fit');
    """)
    snap('library-many-games-narrow')
    route('global');js("click('[data-global-tab=overlay]');click('[data-overlay-editor-tab=metrics]')");pump(200)
    test('Narrow editor tabs retain counts, drafts and inert panels', r"""
        polish.fit();polish.panel('metrics');polish.counts();
        click('[data-preset=Custom]');q('[data-metric=FPS]').click();
        click('[data-overlay-editor-tab=appearance]');polish.panel('appearance');
        polish.fields(q('.overlay-settings-editor'));click('[data-action=discard-global]');
    """)
    snap('overlay-appearance-narrow')
    window.resize(1440, 1000);pump(400)

    js('bridge.failPlayback=true')
    route('replay')
    wait_for("q('#retry-clip')&&!q('#retry-clip').hidden&&q('[data-clip][aria-pressed=true] .thumbnail img')")
    native_motion('[data-action=save-replay]');pump(600)
    test('Replay hint uses one tooltip and suppresses its native title', r"""
        const button=q('[data-action=save-replay]');
        check(qa('#control-hint').length===1,'one custom tooltip');
        check(!button.hasAttribute('title')&&button.dataset.controlTitle.includes('Save last'),'native title migrated');
    """)
    pump(2800)
    test('Runtime polling keeps the tooltip singular and duration changes update its text', r"""
        const button=q('[data-action=save-replay]');
        check(!button.hasAttribute('title')&&qa('#control-hint').length===1,'poll did not restore native tooltip');
        polish.set('#save-duration','60','change');
        check(q('#control-hint').textContent.includes('Save last 60 seconds'),'visible hint follows duration');
        check(!button.hasAttribute('title'),'duration edit keeps native tooltip suppressed');
        polish.set('#save-duration','30','change');
    """)
    native_click('#save-duration+.precision-select-trigger')
    js("window.polishDropdownPageTop=q('.window-content').scrollTop;")
    native_key('End');pump(120)
    test('Long dropdown scrolls its focus plate within the popup only', r"""
        const menu=q('.precision-select-menu'),item=menu.querySelector('.active');
        check(menu.classList.contains('precision-select-scrolls')&&menu.scrollTop>0,'long menu has native scrolling');
        const r=item.getBoundingClientRect(),bounds=menu.getBoundingClientRect();
        check(r.top>=bounds.top&&r.bottom<=bounds.bottom+1,'keyboard choice remains visible');
        const matrix=new DOMMatrix(getComputedStyle(menu,'::before').transform);
        polish.near(matrix.m42,item.offsetTop,'focus plate follows scrolled option');
        check(q('.window-content').scrollTop===polishDropdownPageTop,'document stays stationary');
    """)
    native_key('Escape')
    test('Long dropdown Escape preserves its original selection and focus', r"""
        check(!q('.precision-select-menu')&&q('#save-duration').value==='30','Escape does not commit');
        check(document.activeElement===q('#save-duration+.precision-select-trigger'),'focus stays on trigger');
    """)
    test('Playback failure keeps export unavailable without hiding recovery', r"""
        check(q('#export-clip-button').disabled,'failed playback cannot export');
        check(polish.visible(q('#retry-clip')),'playback recovery visible');
        check(!bridge.calls.some(c=>c.command==='export_replay_clip'),'no export submitted');
        bridge.failPlayback=false;click('#retry-clip');
    """)
    wait_for("q('#clip-video')?.readyState>=2&&!q('#trim-start-bar').disabled", timeout_ms=15000)
    pump(400)
    test('Wide Replay panels share a bottom and menu is last and far right', r"""
        const editor=q('.replay-workspace>.editor'),browser=q('.replay-workspace>.clip-browser');
        polish.near(editor.getBoundingClientRect().bottom,browser.getBoundingClientRect().bottom,'Replay panel bottoms');
        const menu=q('.clip-action-menu'),header=menu.parentElement;
        check(header.lastElementChild===menu,'menu last in header DOM');
        const r=menu.getBoundingClientRect();
        for(const sibling of [...header.children].filter(e=>e!==menu&&polish.visible(e)))
            check(sibling.getBoundingClientRect().right<=r.left+2,'menu far right without overlap');
        const button=q('#export-clip-button');
        check(button.closest('.clip-editor-footer'),'Export belongs to trim footer');
        check(button.textContent.trim()==='Export','semantic Export label');
        check(!polish.visible(button),'unadjusted whole clip hides Export');
        check(q('.clip-editor-footer').getBoundingClientRect().top>=q('.native-player').getBoundingClientRect().bottom-2,'footer below video');
    """)
    test('Selected thumbnails retain their border without a checkmark', r"""
        const selected=q('[data-clip][aria-pressed=true]');check(selected,'selected clip announced');
        const card=selected.closest('.clip-card'),s=getComputedStyle(card);
        check(parseFloat(s.borderTopWidth)>0,'existing selection border retained');
        check(!selected.querySelector('.thumbnail svg'),'loaded selected thumbnail has no decorative check icon');
        for(const thumb of qa('.thumbnail')){
            check(!thumb.querySelector('[class*=check],[data-checkmark]'),'no thumbnail checkmark node');
            check(!/[✓✔☑]/.test(thumb.textContent),'no checkmark text');
            for(const pseudo of ['::before','::after']){
                const p=getComputedStyle(thumb,pseudo);
                check(!/[✓✔☑]/.test(p.content),'no pseudo checkmark');
                check(p.content==='none'||p.content==='normal'||p.display==='none'||
                    !(parseFloat(p.borderRightWidth)>0&&parseFloat(p.borderBottomWidth)>0&&p.transform!=='none'),'no drawn pseudo tick');
            }
        }
    """)
    test('Whole-clip compression remains reachable in the last menu', r"""
        click('.clip-action-menu summary');
        const action=q('.clip-action-menu [data-action=compress-selected-clip]');
        check(polish.visible(action)&&/Compress clip/.test(action.textContent),'whole-clip action visible');
        action.click();check(q('#trim-export-form'),'compression options open');
        check(q('[name=compression]').value==='true','whole-clip semantics retained');
        click('#dialog [data-close]');check(!bridge.calls.some(c=>c.command==='export_replay_clip'),'cancel does not submit');
    """)
    test('Adjusting the trim reveals Export in its original footer', r"""
        q('#trim-start-bar').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true,cancelable:true}));
        const button=q('#export-clip-button');check(polish.visible(button)&&!button.disabled,'trimmed export available');
        check(button.closest('.clip-editor-footer'),'Export stays in footer');
        check(!q('.clip-edit-actions #export-clip-button'),'no header duplicate');
    """)
    snap('replay-trimmed-wide')
    window.resize(640, 800);pump(400)
    test('Narrow Replay keeps trim actions and the last header menu inside the page', r"""
        polish.fit();const footer=q('.clip-editor-footer'),button=q('#export-clip-button');
        check(polish.visible(button)&&button.closest('.clip-editor-footer')===footer,'narrow Export retained');
        for(const element of [button,q('.clip-action-menu')]){
            const r=element.getBoundingClientRect();check(r.left>=0&&r.right<=innerWidth+1,'action fits horizontally');
        }
        click('[data-action=reset-trim]');check(!polish.visible(q('#export-clip-button')),'reset hides whole-clip Export');
        check(bridge.errors.length===0,bridge.errors.join('; '));
        check(!bridge.calls.some(c=>['launch_game','end_session','restart_redunar','export_replay_clip'].includes(c.command)),'no session, restart or export side effects');
    """)
    snap('replay-whole-clip-narrow')
    (ARTIFACTS / 'ui-polish-checks.json').write_text(json.dumps(checks, indent=2)+'\n')
    print(f'PASS: {len(checks)} UI polish checks. Screenshots: {ARTIFACTS}')
except Exception:
    # Keep the first failing production state reviewable, including partial passes.
    (ARTIFACTS / 'ui-polish-partial-checks.json').write_text(json.dumps(checks, indent=2)+'\n')
    diagnostic = js('return JSON.stringify({events:window.polish?.inputEvents,errors:window.bridge?.errors,active:document.activeElement?.outerHTML.slice(0,300)});')
    (ARTIFACTS / 'ui-polish-diagnostic.json').write_text(diagnostic+'\n')
    print('Failure diagnostic: '+diagnostic, flush=True)
    snap('ui-polish-failure')
    raise
finally:
    harness.Handler.do_GET = original_get
    if display:
        x11.XCloseDisplay(display)
    window.destroy()
    server.shutdown()
