"""Replay regressions against the built UI in WebKitGTK, with fake native data.

Run after npm run build:
  xvfb-run -a python3 output/tauri-redunar/tests/replay-webview.py
No production state, native commands, or saved clips are accessed.
"""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import base64
import json
import subprocess
import tempfile
import threading

from webkit_fixture import configure_webkit, configure_webview
configure_webkit()
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2

NATIVE = Path(__file__).resolve().parents[1]
ARTIFACTS = NATIVE.parents[1] / ".redunar-build/reports/replay-ui"
ARTIFACTS.mkdir(parents=True, exist_ok=True)


def pump(milliseconds):
    loop = GLib.MainLoop()
    GLib.timeout_add(milliseconds, lambda: (loop.quit(), False)[1])
    loop.run()


with tempfile.TemporaryDirectory(prefix="redunar-replay-ui-") as temporary:
    fixture = Path(temporary)
    subprocess.run([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
        "testsrc2=s=640x360:r=10", "-t", "3", "-c:v", "libx264",
        "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(fixture / "fixture.mp4"),
    ], check=True)
    subprocess.run([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-i",
        str(fixture / "fixture.mp4"), "-frames:v", "1", str(fixture / "thumb.jpg"),
    ], check=True)
    jpeg = base64.b64encode((fixture / "thumb.jpg").read_bytes()).decode()

    for label, size in [('ultrawide', '840x360'), ('classic', '480x360'), ('portrait', '360x640')]:
        subprocess.run([
            'ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i',
            f'testsrc2=s={size}:r=10', '-t', '3', '-c:v', 'libx264',
            '-pix_fmt', 'yuv420p', '-movflags', '+faststart', str(fixture / f'{label}.mp4'),
        ], check=True)
    # A distinct exported name also receives real decoded trimmed media in this
    # isolated bridge, so discovery cannot accidentally keep playing the source.
    subprocess.run([
        'ffmpeg', '-hide_banner', '-loglevel', 'error', '-ss', '0.25', '-i',
        str(fixture / 'fixture.mp4'), '-t', '2.5', '-c:v', 'libx264',
        '-pix_fmt', 'yuv420p', '-movflags', '+faststart', str(fixture / 'trimmed.mp4'),
    ], check=True)
    media_paths = {f'/{name}.mp4': fixture / f'{name}.mp4'
                   for name in ['fixture', 'trimmed', 'ultrawide', 'classic', 'portrait']}

    class Handler(SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            media_path = media_paths.get(self.path.split("?")[0])
            if media_path is None:
                return super().do_GET()
            body = media_path.read_bytes()
            start, end = 0, len(body) - 1
            requested = self.headers.get("Range")
            if requested:
                first, last = requested.removeprefix("bytes=").split("-", 1)
                start = int(first or 0)
                end = min(int(last) if last else end, end)
            self.send_response(206 if requested else 200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Length", str(end - start + 1))
            if requested:
                self.send_header("Content-Range", f"bytes {start}-{end}/{len(body)}")
            self.end_headers()
            try:
                self.wfile.write(body[start:end + 1])
            except (BrokenPipeError, ConnectionResetError):
                pass  # Selecting a different clip aborts outstanding ranges.

        def translate_path(self, path):
            if path.split("?")[0] == "/fixture.mp4":
                return str(fixture / "fixture.mp4")
            return super().translate_path(path)

    server = ThreadingHTTPServer(("127.0.0.1", 0), partial(Handler, directory=str(NATIVE / "ui/dist")))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_port}"
    manager = WebKit2.UserContentManager()
    setup = """
    window.bridge={calls:[],pending:[],thumbnails:[],hold:false,thumbnailCalls:0,errors:[],exportClips:[],exportEvents:[]};
    window.addEventListener('error',event=>bridge.errors.push(event.message));
    window.addEventListener('unhandledrejection',event=>bridge.errors.push(String(event.reason)));
    window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'}},invoke:async(command,args)=>{
      if(['export_replay_clip','clip_export_status','cancel_clip_export'].includes(command))bridge.exportEvents.push(command);
      if(command==='replay_clips')return [...bridge.exportClips,...Array.from({length:24},(_,i)=>({file_name:i===0?'redunar-replay-1789223687-940555037-1-with-a-very-long-local-recording-filename.mp4':`clip-${i}.mp4`,title:`Clip ${String(i).padStart(2,'0')}`,game_name:i===15?'Fixture game':null,bytes:80000000,modified_unix_ns:'1000000000'}))];
      if(command==='clip_playback_path'){
        bridge.calls.push(args.fileName);
        if(bridge.failPlayback)throw new Error('Fixture preparation failed');
        if(bridge.hold)return new Promise((resolve,reject)=>bridge.pending.push({resolve,reject}));
        return (bridge.mediaPath||bridge.exportClips.find(clip=>clip.file_name===args.fileName)?.media_path||'/fixture.mp4')+'?clip='+args.fileName;
      }
      if(command==='clip_metadata')return {duration_seconds:bridge.exportClips.find(clip=>clip.file_name===args.fileName)?.duration_seconds||3,width:640,height:360,fps:30};
      if(command==='clip_thumbnail'){
        bridge.thumbnailCalls++;
        return new Promise(resolve=>bridge.thumbnails.push(()=>resolve(Array.from(atob(JPEG),c=>c.charCodeAt(0)))));
      }
      if(command==='clip_export_status'&&bridge.failStatus)throw Error('Fixture status unavailable');
      if(command==='clip_export_status'){if(bridge.holdStatus){bridge.holdStatus=false;return new Promise(resolve=>bridge.releaseOldStatus=resolve);}return {active:Boolean(bridge.rejectExport),progress:10,phase:bridge.exportPhase||'analysing',output_bytes:bridge.exportBytes??null};}
      if(command==='cancel_clip_export'){bridge.cancelCalls=(bridge.cancelCalls||0)+1;if(bridge.failCancel)throw Error('Fixture cancellation unavailable');const reject=bridge.rejectExport;if(!reject)throw Error('No clip export is running');bridge.acceptedCancellations=(bridge.acceptedCancellations||0)+1;bridge.rejectExport=null;reject(Error('Clip export was cancelled'));return {};}
      if(command==='export_replay_clip'&&bridge.holdExport){bridge.exported=args;return new Promise((resolve,reject)=>{bridge.rejectPendingExport=reject;if(bridge.holdAdmission)bridge.admitExport=()=>{bridge.rejectExport=reject;};else bridge.rejectExport=reject;});}
      if(command==='export_replay_clip'){
        bridge.exported=args;if(bridge.failExport)throw Error('A clip with that filename already exists');
        const clip={file_name:args.outputName+'.'+args.options.outputFormat,title:args.outputName,duration_seconds:args.endSeconds-args.startSeconds,bytes:1024,modified_unix_ns:'2000000000',media_path:args.endSeconds-args.startSeconds<3?'/trimmed.mp4':'/fixture.mp4'};
        bridge.exportClips.unshift(clip);return {...clip};
      }
      if(command==='replay_storage_status')return {used_bytes:24576};
      if(command==='catalog_games'||command==='session_history')return [];
      if(command==='replay_runtime_status')return {phase:'Unavailable',unavailable_reason:'OpenGL capture is unavailable for this game',completed_save_revision:0};
      if(command==='module_statuses')return {instant_replay:'Enabled'};
      if(command==='app_preferences')return {close_to_tray:false};
      if(command==='replay_preferences')return {initial_save_duration_seconds:30};
      if(command==='global_settings')throw new Error('Unused profile fixture');
      return {};
    }};
    window.check=(condition,message)=>{if(!condition)throw new Error(message)};
    """.replace("JPEG", json.dumps(jpeg))
    manager.add_script(WebKit2.UserScript.new(setup, WebKit2.UserContentInjectedFrames.TOP_FRAME, WebKit2.UserScriptInjectionTime.START, None, None))
    view = WebKit2.WebView.new_with_user_content_manager(manager)
    configure_webview(view)
    window = Gtk.Window()
    window.set_default_size(1400, 1000)
    window.add(view)
    window.show_all()

    def js(source):
        loop = GLib.MainLoop()
        outcome = []

        def complete(webview, result, _):
            try:
                outcome.append(webview.evaluate_javascript_finish(result).to_string())
            except Exception as error:
                outcome.append(error)
            loop.quit()

        view.evaluate_javascript(source, -1, None, None, None, complete, None)
        loop.run()
        if isinstance(outcome[0], Exception):
            raise outcome[0]
        return outcome[0]

    def screenshot(name):
        loop = GLib.MainLoop()

        def snapshot(webview, result, _):
            webview.get_snapshot_finish(result).write_to_png(str(ARTIFACTS / name))
            loop.quit()

        view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, snapshot, None)
        loop.run()

    try:
        view.load_uri(origin + "/#replay")
        pump(1800)
        screenshot("replay-initial.png")
        js("""
        check(document.querySelectorAll('[data-clip]').length===24,'fixture loaded');
        check(document.querySelector('.capture-status-detail').textContent.includes('OpenGL capture is unavailable for this game'),'native replay reason is visible');
        window.rail=document.querySelector('.clip-list');
        check(rail.scrollWidth<=rail.clientWidth+1,'long filename does not overflow rail');
        check(getComputedStyle(document.querySelector('.clip-select strong')).whiteSpace==='normal','filenames wrap');
        check(!document.querySelector('.clip-delete'),'deletion belongs to selected toolbar');
        check(!document.querySelector('.editor').contains(document.querySelector('#save-duration')),'save footer spans workspace');
        """)
        pump(200)
        js("""
        window.search=document.querySelector('#clip-search');
        search.value='clip-1';search.dispatchEvent(new Event('input',{bubbles:true}));
        check(document.querySelectorAll('[data-clip]').length===11,'filtered clips');
        document.querySelector('#save-duration').value='120';
        rail.scrollTop=500;window.railTop=rail.scrollTop;
        const card=document.querySelector('[data-clip="clip-15.mp4"]');
        card.focus({preventScroll:true});card.click();
        check(document.querySelector('.clip-list')===rail,'selection retained rail');
        check(rail.scrollTop===railTop&&railTop>0,'selection retained rail scroll');
        check(document.activeElement===card,'selection retained focus');
        check(search.value==='clip-1','selection retained search');
        check(document.querySelector('#save-duration').value==='120','selection retained save duration');
        check(document.querySelector('[data-selected-clip-title]').textContent==='clip-15.mp4','selected title');
        check(document.querySelector('[data-selected-clip-title]').textContent==='clip-15.mp4','filename caption');
        check(document.querySelector('.native-video-caption strong').textContent==='Fixture game','recorded game label');
        window.selectedVideo=document.querySelector('#clip-video');
        window.beforeCalls=bridge.calls.length;
        card.click();
        check(document.querySelector('#clip-video')===selectedVideo,'same clip retained player');
        check(bridge.calls.length===beforeCalls,'same clip did not prepare twice');
        bridge.thumbnails.splice(0).forEach(resolve=>resolve());
        """)
        pump(1000)
        js("""
        bridge.thumbnails.splice(0).forEach(resolve=>resolve());
        check(document.querySelector('.clip-list')===rail&&rail.scrollTop===railTop,'thumbnail retained rail scroll');
        check(document.querySelector('#clip-video')===selectedVideo,'thumbnail retained player');
        check(!document.querySelector('#clip-play-toggle').disabled,'media ready');
        check(document.querySelector('#export-clip-button').hidden,'full recording hides the footer Export action');
        window.toggleIcon=document.querySelector('#clip-play-toggle svg');
        window.exportIcon=document.querySelector('#export-clip-button svg');
        selectedVideo.dispatchEvent(new Event('timeupdate'));
        check(document.querySelector('#clip-play-toggle svg')===toggleIcon,'idle update retained play icon');
        check(document.querySelector('#export-clip-button svg')===exportIcon,'idle update retained export icon');
        """)
        js("""
        (()=>{
         const menu=document.querySelector('.clip-action-menu'),summary=menu.querySelector('summary'),first=menu.querySelector('button'),search=document.querySelector('#clip-search');
         summary.click();first.focus();
         const escape=new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true});first.dispatchEvent(escape);
         check(!menu.open&&escape.defaultPrevented,'Escape dismisses the clip menu');
         check(document.activeElement===summary,'Escape restores focus to the clip menu trigger');
         summary.click();first.focus();
         search.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));search.focus();search.click();
         check(!menu.open&&document.activeElement===search,'outside control keeps its own focus');
         summary.click();first.focus();
         document.querySelector('#clip-video').dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));
         check(!menu.open&&document.activeElement===summary,'outside picture dismissal does not strand focus in hidden menu');
        })();
        """)
        pump(250)
        js("check(!document.querySelector('.clip-action-menu').open,'a late disclosure animation cannot reopen the dismissed clip menu')")
        # The selection made during the first thumbnail batch is retried once.
        pump(1000)
        js("bridge.thumbnails.splice(0).forEach(resolve=>resolve())")
        pump(150)
        js("""
        check(document.querySelector('[data-clip="clip-15.mp4"] img'),'selected thumbnail patched');
        check(document.querySelector('[data-clip="clip-15.mp4"] [data-clip-duration]').textContent==='00:03','real duration badge');
        check(document.querySelector('.clip-list')===rail&&rail.scrollTop===railTop,'visible image retained rail scroll');
        bridge.hold=true;bridge.calls=[];
        document.querySelector('[data-clip="clip-16.mp4"]').click();
        document.querySelector('[data-clip="clip-17.mp4"]').click();
        document.querySelector('[data-clip="clip-18.mp4"]').click();
        check(bridge.calls.join(',')==='clip-16.mp4','bounded preparation');
        check(document.querySelector('[data-selected-clip-title]').textContent==='clip-18.mp4','selection updates while native preparation pending');
        check(document.querySelector('#clip-play-toggle').disabled,'old media readiness cleared');
        check(document.querySelector('#export-clip-button').hidden,'pending selection hides footer Export');
        bridge.pending.shift().resolve('/fixture.mp4?obsolete');
        """)
        pump(100)
        js("""
        check(bridge.calls.join(',')==='clip-16.mp4,clip-18.mp4','intermediate preparation skipped');
        check(!document.querySelector('#clip-video').hasAttribute('src'),'stale URL discarded');
        bridge.pending.shift().resolve('/fixture.mp4?current');
        """)
        pump(500)
        js("""
        check(document.querySelector('#clip-video').getAttribute('src')==='/fixture.mp4?current','newest URL loaded');
        check(rail.scrollTop===railTop,'rapid selection retained scroll');
        bridge.hold=false;
        """)

        # The window content scrolls independently from the document and rail.
        window.resize(900, 850)
        pump(300)
        screenshot("replay-narrow.png")
        js("""
        window.workspaceScroll=document.querySelector('.window-content');
        const next=document.querySelector('[data-clip="clip-19.mp4"]');
        next.scrollIntoView({block:'center'});window.pageTop=workspaceScroll.scrollTop;
        next.click();
        check(pageTop>0&&workspaceScroll.scrollTop===pageTop,'selection retained workspace scroll '+JSON.stringify({pageTop,after:workspaceScroll.scrollTop,height:workspaceScroll.scrollHeight,client:workspaceScroll.clientHeight,documentTop:scrollY}));
        """)
        pump(600)
        js("check(workspaceScroll.scrollTop===pageTop,'metadata retained workspace scroll');check(bridge.errors.length===0,bridge.errors.join(';'))")
        js("""
        bridge.hold=true;
        document.querySelector('[data-clip="clip-18.mp4"]').click();
        window.detachedVideo=document.querySelector('#clip-video');
        location.hash='overview';
        """)
        pump(100)
        js("bridge.pending.shift().reject('obsolete failure');bridge.hold=false")
        pump(100)
        js("""
        check(document.body.dataset.page==='overview','navigation works during preparation');
        check(document.querySelector('#toast').hidden,'stale preparation error stayed hidden');
        location.hash='replay';
        """)
        pump(400)
        js("""
        check(!document.querySelector('#clip-play-toggle').disabled,'return to replay loads media');
        const current=document.querySelector('#clip-video');
        const before=document.querySelector('#clip-time').textContent;
        detachedVideo.dispatchEvent(new Event('seeked'));
        check(document.querySelector('#clip-time').textContent===before,'old media event ignored');
        document.querySelector('#clip-search').value='clip-1';
        document.querySelector('#clip-search').dispatchEvent(new Event('input',{bubbles:true}));
        window.rail=document.querySelector('.clip-list');
        check(bridge.errors.length===0,bridge.errors.join(';'));
        """)
        for _ in range(30):
            if js("document.querySelector('#clip-filmstrip').dataset.frames||''") == '8':
                break
            pump(200)
        js("""
        const filmstrip=document.querySelector('#clip-filmstrip');
        check(filmstrip.dataset.frames==='8','eight bounded frame previews');
        const context=filmstrip.getContext('2d');
        const first=Array.from(context.getImageData(0,0,160,90).data);
        const last=Array.from(context.getImageData(1120,0,160,90).data);
        check(first.some((channel,index)=>channel!==last[index]),'filmstrip samples different clip moments '+JSON.stringify({first:first.slice(0,8),last:last.slice(0,8),sum:first.reduce((a,b)=>a+b,0),width:filmstrip.width,height:filmstrip.height}));
        check(document.querySelector('#clip-video').currentTime===0,'sampling did not seek player');
        const start=document.querySelector('#trim-start-bar'),end=document.querySelector('#trim-end-bar');
        start.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}));
        end.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true}));
        check(start.getAttribute('aria-valuenow')==='0.25','keyboard trim start');
        check(end.getAttribute('aria-valuenow')==='2.75','keyboard trim end');
        check(document.querySelector('#clip-selected-duration').textContent==='00:02.5','selected duration follows trim');
        check(document.querySelector('#trim-start-value').textContent==='00:00.3','visible In follows trim');
        check(document.querySelector('#clip-filmstrip')===filmstrip,'trimming retained filmstrip');
        check(!document.querySelector('#export-clip-button').hidden&&!document.querySelector('#export-clip-button').disabled,'adjusted decoded trim shows Export');
        check(document.querySelector('#export-clip-button').textContent.trim()==='Export','quiet footer label');
        document.querySelector('#export-clip-button').click();
        const form=document.querySelector('#trim-export-form');
        check(form.elements.start.value==='0.25'&&form.elements.end.value==='2.75','exact export boundaries');
        check(form.querySelector('.clip-export-original').textContent==='Original · 80.00 MB','measured source size');
        check(document.querySelector('#dialog-title').textContent==='Export clip','trimmed export heading');
        check(form.elements.copyRange.value==='selection','trim opens the selected range');
        form.elements.copyRange.value='entire';form.elements.copyRange.dispatchEvent(new Event('change',{bubbles:true}));
        check(form.elements.start.value==='0'&&form.elements.end.value==='3','entire copy ignores editor trim');
        check(form.elements.compression.value==='false','uncapped whole copy does not request native compression');
        form.elements.copyRange.value='selection';form.elements.copyRange.dispatchEvent(new Event('change',{bubbles:true}));
        check(form.elements.start.value==='0.25'&&form.elements.end.value==='2.75','returning to selection restores exact editor snapshot');
        check(!form.elements.limitSize.checked&&form.elements.limitSize.getAttribute('aria-expanded')==='false','trim compression starts folded');
        check(form.elements.maxSizeMb.disabled,'folded size is not submitted');
        form.elements.limitSize.click();
        check(form.querySelector('.clip-export-size-comparison').textContent.includes('Selection ≈ 66.67 MB'),'selection estimate uses full source duration');
        check(form.elements.maxSizeMb.max==='66.6'&&Number(form.elements.maxSizeMb.value)<66.6,'selection maximum and default reduce its estimated size');
        form.elements.maxSizeMb.value='0.7';form.elements.maxSizeMb.dispatchEvent(new Event('input',{bubbles:true}));
        form.elements.outputFormat.value='mkv';form.elements.outputFormat.dispatchEvent(new Event('change',{bubbles:true}));
        check(form.querySelector('[data-export-extension]').textContent==='.mkv','format updates suffix');
        form.elements.limitSize.click();
        check(form.elements.maxSizeMb.disabled&&form.elements.maxSizeMb.value==='0.7','folding preserves value but disables size');
        check(form.querySelector('[data-export-size-controls]').inert,'collapsed compression is inert');
        check(form.elements.compression.value==='false','selected size limiting never requests the whole native source');
        form.elements.limitSize.click();
        form.elements.outputName.value='My named trim';bridge.failExport=true;form.requestSubmit();
        """)
        pump(350)
        js("check(document.querySelector('#dialog').open,'export collision retains dialog');check(document.querySelector('#dialog-body').textContent.includes('already exists'),'collision explanation');check(document.querySelector('[name=outputName]').value==='My named trim','filename retained for retry');bridge.failExport=false;document.querySelector('#trim-export-form').requestSubmit();")
        pump(350)
        js("check(bridge.exported.startSeconds===.25&&bridge.exported.endSeconds===2.75,'export sent exact range');check(bridge.exported.outputName==='My named trim','filename stem sent');check(!('destination' in bridge.exported),'native owns configured folder');check(bridge.exported.options.maxSizeMb===.7&&bridge.exported.options.outputFormat==='mkv'&&!bridge.exported.options.compression,'trimmed export retains options on retry');check(document.querySelector('[data-action=show-export-folder]')&&document.querySelector('[data-action=open-exported-clip]'),'completion actions');check(document.querySelector('#dialog-title').textContent==='Clip exported','export completion heading')")
        pump(700)
        js("""
        (()=>{
         const output='My named trim.mkv';
         check(document.querySelector('[data-selected-clip-title]').textContent===output,'native completion selects the distinct exported filename');
         check(document.querySelector('[data-action=open-exported-clip]').dataset.fileName===output,'completion opens the new output');
         const search=document.querySelector('#clip-search');search.value='';search.dispatchEvent(new Event('input',{bubbles:true}));
         check(document.querySelectorAll('[data-clip]').length===25,'inventory refresh discovers the extra exported clip');
         const added=Array.from(document.querySelectorAll('[data-clip]')).find(card=>card.dataset.clip===output);
         check(added?.getAttribute('aria-pressed')==='true','new output is selected in the refreshed rail');
         check(document.querySelector('[data-clip="clip-18.mp4"]'),'source recording remains in the inventory');
         document.querySelector('[data-action=open-exported-clip]').click();
        })();
        """)
        pump(500)
        js("""
        (()=>{
         const video=document.querySelector('#clip-video');
         check(video.dataset.clipId==='My named trim.mkv'&&video.getAttribute('src').startsWith('/trimmed.mp4'),'Open clip uses the exported media, not the source');
         check(Math.abs(video.duration-2.5)<.05&&video.readyState>=2,'distinct output has real decoded trimmed duration');
         const search=document.querySelector('#clip-search');search.value='clip-1';search.dispatchEvent(new Event('input',{bubbles:true}));
         document.querySelector('[data-clip="clip-18.mp4"]').click();
        })();
        """)
        pump(600)
        js("""
        document.querySelector('#trim-start-bar').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',shiftKey:true,bubbles:true}));
        check(Number(document.querySelector('#trim-start-bar').getAttribute('aria-valuenow'))<=2.9,'trim handles cannot cross');
        document.querySelector('#trim-start-bar').dispatchEvent(new KeyboardEvent('keydown',{key:'Home',bubbles:true}));
        document.querySelector('#trim-end-bar').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true}));
        check(bridge.errors.length===0,bridge.errors.join(';'));
        """)
        js("""
        check(document.querySelector('#clip-video').preload==='auto','normal video preloading');
        window.retryRail=document.querySelector('.clip-list');
        retryRail.scrollTop=400;window.retryTop=retryRail.scrollTop;
        document.querySelector('#clip-video').dispatchEvent(new Event('error'));
        check(!document.querySelector('#retry-clip').hidden,'media failure offers retry');
        check(document.querySelector('#clip-play-toggle').disabled,'failed player is disabled');
        check(document.querySelector('#export-clip-button').hidden,'failed decoding hides footer Export');
        check(!document.querySelector('#clip-video').hasAttribute('src'),'failed stream released');
        document.querySelector('#media-state-overlay').scrollIntoView({block:'center'});
        """)
        screenshot("replay-load-failure.png")
        js("""
        document.querySelector('.clip-action-menu summary').click();
        document.querySelector('[data-action=compress-selected-clip]').click();
        check(document.querySelector('#dialog-title').textContent==='Compress clip','legacy compression works from selected source metadata without decoded readiness');
        check(document.querySelector('[name=start]').value==='0'&&document.querySelector('[name=end]').value==='3','unready whole-source compression uses its own duration');
        check(document.querySelector('[name=compression]').value==='true'&&!document.querySelector('[name=maxSizeMb]').disabled,'legacy compression starts checked and ready');
        document.querySelector('#dialog [data-close]').click();
        """)
        js("""
        bridge.failPlayback=true;document.querySelector('#retry-clip').click();
        """)
        pump(100)
        js("""
        check(document.querySelector('#media-state-detail').textContent==='Fixture preparation failed','preparation failure shown');
        check(!document.querySelector('#retry-clip').hidden,'preparation failure remains retryable');
        bridge.failPlayback=false;document.querySelector('#retry-clip').click();
        """)
        pump(400)
        js("""
        check(document.querySelector('#retry-clip').hidden,'successful retry clears failure');
        check(!document.querySelector('#clip-play-toggle').disabled,'retry loads playable video');
        check(document.querySelector('.clip-list')===retryRail&&retryRail.scrollTop===retryTop,'retry retains rail and scroll');
        """)
        window.resize(1400, 1000)
        pump(300)
        js("scrollTo(0,150);document.querySelector('.clip-list').scrollTop=500")
        pump(200)
        screenshot("replay-webview.png")
        js("document.querySelector('#trim-start-bar').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}));document.querySelector('#export-clip-button').click()")
        pump(100)
        screenshot("export-dialog.png")
        js("check(document.querySelector('[name=outputName]'),'editable export filename');check(!document.querySelector('[name=limitSize]').checked&&document.querySelector('[name=limitSize]').getAttribute('aria-expanded')==='false','Compress copy starts off');check(document.querySelector('.clip-export-range').textContent.includes('Selected range'),'selection shown');document.querySelector('#dialog [data-close]').click();const seek=document.querySelector('#player-seek');check(!seek.disabled,'seek enabled');seek.value='1';seek.dispatchEvent(new Event('input',{bubbles:true}));")
        pump(200)
        js("check(Math.abs(document.querySelector('#clip-video').currentTime-1)<.1,'precision seek reaches requested time')")
        js("const selected=document.querySelector('[data-selected-clip-title]').textContent;document.querySelector('[data-action=delete-selected-clip]').click();check(JSON.parse(document.querySelector('#trash-clips-form [name=fileNames]').value)[0]===selected,'delete targets current selection');document.querySelector('#dialog [data-close]').click();")
        for viewport in [1400, 900, 640, 360]:
            window.resize(viewport, 1000)
            pump(200)
            js("""
            (()=>{
             document.querySelector('[data-action=reset-trim]').click();
             const button=document.querySelector('#export-clip-button'),footer=document.querySelector('.clip-editor-footer');
             check(button.hidden&&getComputedStyle(button).display==='none','full-range Export has no visible disabled affordance');
             window.footerGeometry={footer:footer.getBoundingClientRect().height,editor:document.querySelector('.editor').getBoundingClientRect().height,timeline:document.querySelector('#clip-timeline').getBoundingClientRect().top,video:document.querySelector('#clip-video')};
             document.querySelector('#trim-start-bar').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}));
            })();
            """)
            pump(100)
            js("""
            (()=>{
             check(!document.querySelector('#export-clip-button').hidden,'trim reveals Export at each viewport');
             check(Math.abs(document.querySelector('.clip-editor-footer').getBoundingClientRect().height-footerGeometry.footer)<1,'revealing Export retains the reserved footer height');
             check(Math.abs(document.querySelector('.editor').getBoundingClientRect().height-footerGeometry.editor)<1,'revealing Export retains editor geometry');
             check(Math.abs(document.querySelector('#clip-timeline').getBoundingClientRect().top-footerGeometry.timeline)<1,'revealing Export does not move the timeline');
             check(document.querySelector('#clip-video')===footerGeometry.video,'responsive footer does not replace decoded media');
            })();
            """)
        window.resize(1400, 1000)
        pump(200)
        js("const resetStartHandle=document.querySelector('#trim-start-bar');resetStartHandle.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}));check(!document.querySelector('[data-action=reset-trim]').disabled,'trim reset available');document.querySelector('[data-action=reset-trim]').click();check(document.querySelector('#trim-start-value').textContent==='00:00.0','trim starts at zero after reset');check(document.querySelector('#clip-selected-duration').textContent==='00:03.0','reset retains entire recording');check(document.querySelector('#export-clip-button').hidden,'reset to full recording hides Export again');check(!document.querySelector('#center-play').hidden,'center play visible while paused');document.querySelector('#center-play').click();")
        pump(300)
        js("check(!document.querySelector('#clip-video').paused,'center play starts video');check(document.querySelector('#center-play').hidden,'center control leaves playing picture clear');document.querySelector('#clip-play-toggle').click();")
        js("(()=>{check(document.querySelector('#export-clip-button').hidden,'whole-source compression does not need footer Export');const menu=document.querySelector('.clip-action-menu');menu.querySelector('summary').click();check(menu.open,'far-right clip actions opened');const compress=menu.querySelector('[data-action=compress-selected-clip]');check(compress.textContent.trim()==='Compress clip…','legacy compression action restored');compress.click();check(!menu.open,'compression closes the ellipsis menu');})()")
        pump(250)
        js("check(document.querySelector('#dialog-title').textContent==='Compress clip','whole-clip compression heading');check(document.querySelector('[name=start]').value==='0'&&document.querySelector('[name=end]').value==='3','whole recording ignores trim');check(document.querySelector('[name=copyRange]').value==='entire'&&document.querySelector('[name=limitSize]').checked,'entire clip with Compress copy selected');check(document.querySelector('[name=compression]').value==='true','whole-source size limiting requests native inspected duration');check(!document.querySelector('[name=maxSizeMb]').disabled,'maximum size enabled');check(document.querySelector('[name=outputFormat]').value==='mp4','source container default')")
        js("(()=>{const form=document.querySelector('#trim-export-form'),field=form.elements.maxSizeMb,slider=form.querySelector('[data-export-size-slider]');check(field.max==='80'&&field.value==='40','whole clip starts at half its measured size');check(slider.max==='80','slider stops at original size');slider.value='80';slider.dispatchEvent(new Event('input',{bubbles:true}));check(form.querySelector('[type=submit]').disabled&&form.querySelector('#export-size-status').textContent.includes('Already under this limit'),'no-op compression explains that it already fits');field.value='100';field.dispatchEvent(new Event('change',{bubbles:true}));check(field.value==='80','typed size above source clamps on commit');slider.value='40';slider.dispatchEvent(new Event('input',{bubbles:true}));check(!form.querySelector('[type=submit]').disabled&&form.querySelector('[data-export-limit-label]').textContent==='40 MB','smaller size restores compression');})()")
        screenshot('compress-clip-dialog.png')
        window.resize(360,800);pump(250)
        js("const dialog=document.querySelector('#dialog');check(dialog.scrollWidth<=dialog.clientWidth,'narrow compression dialog fits')")
        screenshot('compress-clip-narrow.png')
        window.resize(1400,1000);pump(250)
        js("bridge.holdStatus=true;bridge.failExport=true;document.querySelector('#trim-export-form').requestSubmit()")
        pump(150)
        js("bridge.failExport=false;bridge.holdExport=true;document.querySelector('#trim-export-form').requestSubmit();bridge.releaseOldStatus({active:true,progress:99})")
        pump(100)
        js("(()=>{const working=document.querySelector('[data-export-working]');check(working.disabled&&working.getAttribute('aria-busy')==='true','busy button prevents another export');check(document.querySelector('#dialog-title').textContent==='Compressing clip','retry replaces the failure heading while working');check(working.querySelector('.clip-export-spinner')&&!working.querySelector('svg'),'simple spinner without a checkmark');check(document.querySelector('.clip-export-progress span').textContent==='Exporting','shared progress label');check(document.querySelector('.clip-export-source strong').textContent==='clip-18.mp4','progress keeps source filename');})()")
        js("check(document.querySelector('#clip-export-progress').value!==99,'stale status cannot repaint a later export');check(document.querySelector('#clip-export-output').textContent==='Analysing clip','analysis has no misleading file size');check(document.querySelector('.clip-export-original').textContent==='Original · 80.00 MB','original size retained during encoding');bridge.exportPhase='encoding';bridge.exportBytes=1234567;")
        pump(350)
        screenshot('export-spinner-progress.png')
        window.resize(360,800);pump(250)
        js("(()=>{const dialog=document.querySelector('#dialog');check(dialog.scrollWidth<=dialog.clientWidth,'narrow progress dialog fits');const cancel=document.querySelector('[data-export-cancel]').getBoundingClientRect(),working=document.querySelector('[data-export-working]').getBoundingClientRect();check(cancel.right<=working.left,'narrow buttons stay separate');})()")
        screenshot('export-spinner-narrow.png')
        window.resize(1400,1000);pump(250)
        js("check(document.querySelector('#clip-export-output').textContent==='Exporting clip · 1.23 MB','current measured output size');bridge.exportPhase='analysing';bridge.exportBytes=9876543;")
        pump(350)
        js("check(document.querySelector('#clip-export-output').textContent==='Analysing clip','retry analysis hides previous output bytes');const event=new Event('cancel',{cancelable:true});document.querySelector('#dialog').dispatchEvent(event);check(event.defaultPrevented,'active export retains cancel dialog');bridge.failStatus=true;bridge.failCancel=true;document.querySelector('[data-export-cancel]').click();document.querySelector('[data-export-cancel]').click()")
        pump(350)
        js("""
        check(bridge.cancelCalls===1,'overlapping clicks issue only one cancellation attempt');
        check(bridge.rejectExport,'failed cancellation leaves the native export pending');
        check(!document.querySelector('[data-export-cancel]').disabled&&document.querySelector('[data-export-cancel]').textContent==='Retry cancellation','failed cancellation offers an enabled retry');
        check(!document.querySelector('#dialog-error').hidden&&document.querySelector('#dialog-error').textContent.includes('Fixture cancellation unavailable'),'cancellation failure explains retry');
        check(document.querySelector('#clip-export-output').textContent.includes('status unavailable'),'unavailable polling is reported independently');
        bridge.failCancel=false;document.querySelector('[data-export-cancel]').click();
        """)
        pump(250)
        js("check(bridge.cancelCalls===2&&bridge.acceptedCancellations===1,'retry reaches the native job once despite failed status polling');check(document.querySelector('#dialog-title').textContent==='Export cancelled','cancelled export returns to choices');check(!document.querySelector('[data-export-working]'),'cancellation removes busy feedback');check(document.querySelector('[name=compression]').value==='true','cancellation retains whole-clip mode');check(document.querySelector('[name=maxSizeMb]').value==='40','cancellation retains size');check(document.querySelector('[name=copyRange]').value==='entire'&&document.querySelector('[name=limitSize]').checked,'cancellation retains entire compressed copy choices');check(bridge.exported.startSeconds===0&&bridge.exported.endSeconds===3&&bridge.exported.options.compression===true&&bridge.exported.options.maxSizeMb===40,'whole export sends the actual native whole-source arguments');bridge.failStatus=false;bridge.holdStatus=true;document.querySelector('#trim-export-form').requestSubmit()")
        pump(100)
        js("check(bridge.releaseOldStatus,'current status request is held unresolved');document.querySelector('[data-export-cancel]').click()")
        pump(250)
        js("check(bridge.cancelCalls===3&&bridge.acceptedCancellations===2,'an unresolved status request cannot block cancellation');check(document.querySelector('#dialog-title').textContent==='Export cancelled','held-status cancellation resolves normally');bridge.releaseOldStatus({active:true,progress:99});bridge.holdAdmission=true;bridge.failStatus=true;bridge.exportEvents=[];document.querySelector('#trim-export-form').requestSubmit()")
        pump(50)
        js("""
        check(bridge.exportEvents[0]==='export_replay_clip','native export invocation precedes its first status read');
        check(!bridge.rejectExport&&bridge.admitExport,'native admission remains deliberately pending');
        document.querySelector('[data-export-cancel]').click();
        """)
        pump(75)
        js("check(bridge.cancelCalls===4,'early cancellation is attempted immediately before admission');check(document.querySelector('[data-export-cancel]').disabled&&document.querySelector('[data-export-cancel]').textContent==='Cancelling…','early no-job response retains cancel intent');bridge.admitExport()")
        pump(350)
        js("check(bridge.cancelCalls===5&&bridge.acceptedCancellations===3,'retained early cancellation reaches the admitted job despite failed status reads');check(document.querySelector('#dialog-title').textContent==='Export cancelled','early cancellation resolves normally');bridge.exportEvents=[];document.querySelector('#trim-export-form').requestSubmit()")
        pump(50)
        js("document.querySelector('[data-export-cancel]').click()")
        pump(75)
        js("check(bridge.cancelCalls===6,'second early request has a queued admission retry');bridge.rejectPendingExport(Error('Fixture admission failed'))")
        pump(75)
        js("bridge.holdAdmission=false;bridge.failStatus=false;document.querySelector('#trim-export-form').requestSubmit()")
        pump(350)
        js("check(bridge.cancelCalls===6&&bridge.rejectExport,'old admission retry cannot cancel a later export');document.querySelector('[data-export-cancel]').click()")
        pump(200)
        js("check(bridge.cancelCalls===7&&bridge.acceptedCancellations===4,'later export receives only its own explicit cancellation');bridge.holdExport=false;document.querySelector('#trim-export-form').requestSubmit()")
        pump(350)
        js("check(document.querySelector('#dialog-title').textContent==='Clip compressed','legacy compression completion heading');check(document.querySelector('[data-action=show-export-folder]')&&document.querySelector('[data-action=open-exported-clip]'),'compression completion actions');document.querySelector('#dialog [data-close]').click()")
        # Exercise actual decoded display dimensions, including a landscape
        # thumbnail on non-landscape recordings, rather than mocking videoWidth.
        for label, width, height in [('fixture', 640, 360), ('ultrawide', 840, 360), ('classic', 480, 360), ('portrait', 360, 640)]:
            js(f"bridge.mediaPath='/{label}.mp4';document.querySelector('[data-action=retry-clip]').click();")
            pump(700)
            for viewport in [1400, 640]:
                window.resize(viewport, 1000)
                pump(250)
                js(f"""
                {{
                const video=document.querySelector('#clip-video'),stage=video.parentElement;
                const rect=video.getBoundingClientRect(),box=stage.getBoundingClientRect();
                check(video.videoWidth==={width}&&video.videoHeight==={height},'decoded {label} dimensions');
                check(Math.abs(rect.height-rect.width*{height}/{width})<1,'native {label} aspect at {viewport}');
                check(Math.abs(box.height-rect.height)<1&&Math.abs(box.width-rect.width)<1,'video fills stage');
                check(getComputedStyle(video).objectFit==='contain','whole frame preserved');
                check(parseFloat(getComputedStyle(stage).borderTopLeftRadius)>0&&getComputedStyle(stage).overflow==='hidden','rounded image corners');
                check(Math.abs(document.querySelector('.player-controls').getBoundingClientRect().top-box.bottom)<2,'controls directly below clip');
                check(document.documentElement.scrollWidth<=innerWidth+1,'no horizontal overflow');
                }}
                """)
                if viewport==1400:
                    js("scrollTo(0,150)")
                    screenshot(f'replay-aspect-{label}.png')
        window.resize(1400,1000);pump(200)
        js("bridge.mediaPath='/ultrawide.mp4';window.aspectViewport=document.querySelector('.window-content');aspectViewport.scrollTop=100;window.aspectScroll=aspectViewport.scrollTop;window.aspectRail=document.querySelector('.clip-list');aspectRail.scrollTop=400;window.aspectRailTop=aspectRail.scrollTop;document.querySelector('[data-clip=\"clip-15.mp4\"]').focus({preventScroll:true});document.querySelector('[data-clip=\"clip-15.mp4\"]').click();")
        pump(700)
        js("check(aspectViewport.scrollTop===Math.min(aspectScroll,aspectViewport.scrollHeight-aspectViewport.clientHeight),'aspect change retains workspace scroll within available content');check(document.querySelector('.clip-list')===aspectRail&&aspectRail.scrollTop===aspectRailTop,'aspect change retains rail scroll');check(bridge.errors.length===0,bridge.errors.join(';')); ")
        print('PASS: WebKitGTK rail/document scroll, filter, focus, save duration, thumbnails, readiness, unchanged control icons, rapid selection, eight real frame samples, keyboard trimming, stable conditional Export, clip menu dismissal/focus, distinct exported media, legacy ellipsis compression, shared ranges, format/size validation, retry snapshots, stale export status, cancellation retries despite failed/unresolved polls and decoded aspect ratios.')
    finally:
        window.destroy()
        server.shutdown()
