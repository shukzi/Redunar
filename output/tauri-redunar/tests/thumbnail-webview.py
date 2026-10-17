"""Cached card images paint independently of metadata and survive UI refreshes."""
from webview_fixture import *

extra=r"""
bridge.idle=true;bridge.automaticUpdates=false;bridge.pendingMetadata=[];
const invoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='clip_metadata'&&!bridge.releaseMetadata){
  await new Promise(resolve=>bridge.pendingMetadata.push(resolve));
 }
 return invoke(command,args);
};
window.thumbnails=()=>bridge.calls.filter(c=>c.command==='clip_thumbnail').length;
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#overview');pump(1200)
    wait_for("qa('.recent-thumbnail img').length>=2&&qa('.recent-thumbnail img').every(image=>image.complete&&image.naturalWidth>0)")
    test('Cached previews paint while metadata remains pending',"check(bridge.pendingMetadata.length>0,'inspection held');check(thumbnails()>=2,'images requested independently');check(qa('.recent-thumbnail img').every(image=>image.complete&&image.naturalWidth>0),'cached images visible');bridge.releaseMetadata=true;bridge.pendingMetadata.splice(0).forEach(resolve=>resolve());")
    pump(300)
    wait_for("qa('.recent-thumbnail img').length===3")
    js("q('.recent-captures').scrollIntoView({block:'center'})");pump(100)
    snap('thumbnail-cache-ready')
    js("window.reads=thumbnails()")
    route('library');route('overview')
    test('Returning to Overview reuses mounted-data thumbnails immediately',"check(qa('.recent-thumbnail img').length===3,'no placeholder on return');check(thumbnails()===reads,'no thumbnail regeneration');")
    route('replay');pump(1000)
    test('Replay and inventory refresh reuse unchanged thumbnails',"check(qa('.thumbnail img').length===3,'all card images retained');window.reads=thumbnails();click('[data-action=reload-clips]');")
    pump(1100)
    test('Reloading unchanged inventory keeps cached images',"check(qa('.thumbnail img').length===3,'images retained after refresh');check(thumbnails()===reads,'unchanged files not requested again');bridge.clips[0].bytes++;click('[data-action=reload-clips]');")
    pump(1300)
    test('Changed clip identity requests a fresh preview only for that clip',"check(thumbnails()===reads+1,'one invalidated clip');check(qa('.thumbnail img').length===3,'fresh preview visible');check(bridge.errors.length===0,bridge.errors.join('; '));")
    (ARTIFACTS/'thumbnail-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} thumbnail UI checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
