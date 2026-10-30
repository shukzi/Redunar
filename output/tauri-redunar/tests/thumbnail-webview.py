"""Cached card images paint independently of metadata and survive UI refreshes."""
from webview_fixture import *

extra=r"""
bridge.idle=true;bridge.automaticUpdates=false;bridge.pendingMetadata=[];
bridge.previewEntry={mounted:null,firstRequest:null};
if(location.search==='?replay-entry')bridge.clips=Array.from({length:8},(_,i)=>({file_name:`cached-${i}.mkv`,bytes:68000000,modified_unix_ns:'1789300000000000000',game_name:null,favorite:false}));
new MutationObserver(()=>{
 if(bridge.previewEntry.mounted===null&&(q('.recent-captures')||q('.clip-list')))bridge.previewEntry.mounted=performance.now();
}).observe(document,{childList:true,subtree:true});
const invoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='clip_thumbnail'&&bridge.previewEntry.firstRequest===null)bridge.previewEntry.firstRequest=performance.now();
 if(command==='clip_thumbnail'&&bridge.holdFile===args.fileName){
  const bytes=bridge.thumbnailBytes;
  await new Promise(resolve=>bridge.heldThumbnails.push(resolve));
  return bytes;
 }
 if(command==='clip_metadata'&&!bridge.releaseMetadata){
  await new Promise(resolve=>bridge.pendingMetadata.push(resolve));
 }
 return invoke(command,args);
};
window.thumbnails=()=>bridge.calls.filter(c=>c.command==='clip_thumbnail').length;
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#overview')
    wait_for("window.bridge?.previewEntry?.firstRequest!=null")
    test('Cache requests start on entry without the old idle delay',"bridge.previewEntry.requestDelayMs=bridge.previewEntry.firstRequest-bridge.previewEntry.mounted;check(bridge.previewEntry.requestDelayMs>=0&&bridge.previewEntry.requestDelayMs<400,'entry delay '+bridge.previewEntry.requestDelayMs+' ms');")
    wait_for("qa('.recent-thumbnail img').length===3&&qa('.recent-thumbnail img').every(image=>image.complete&&image.naturalWidth>0)")
    test('Every visible cached preview paints while metadata remains pending',"check(bridge.pendingMetadata.length===2,'metadata concurrency bounded');check(thumbnails()===3,'entire visible queue drained');check(qa('.recent-thumbnail img').every(image=>image.complete&&image.naturalWidth>0),'cached images visible');bridge.releaseMetadata=true;bridge.pendingMetadata.splice(0).forEach(resolve=>resolve());")
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
    overview_timing=json.loads(js('return JSON.stringify(bridge.previewEntry)'))
    view.load_uri(f'http://127.0.0.1:{server.server_port}/?replay-entry#library')
    wait_for("window.q&&q('#library-list')&&qa('.library-game').length>0")
    js("location.hash='replay'")
    wait_for("bridge.previewEntry.firstRequest!==null")
    test('Entering Instant Replay starts cache reads without an idle delay',"bridge.previewEntry.requestDelayMs=bridge.previewEntry.firstRequest-bridge.previewEntry.mounted;check(bridge.previewEntry.requestDelayMs>=0&&bridge.previewEntry.requestDelayMs<400,'replay entry delay '+bridge.previewEntry.requestDelayMs+' ms');window.visibleCards=()=>{const bounds=q('.clip-list').getBoundingClientRect();return qa('[data-clip]').filter(card=>{const rect=card.getBoundingClientRect();return rect.bottom>=bounds.top&&rect.top<=bounds.bottom;});};check(visibleCards().length>=2,'visible cards present');")
    wait_for("visibleCards().every(card=>card.querySelector('img')?.naturalWidth>0)")
    test('All visible Replay images load despite stalled metadata',"check(bridge.pendingMetadata.length===2,'only two metadata reads');check(thumbnails()===visibleCards().length,'only visible images read');q('.clip-list').scrollTop=q('.clip-list').scrollHeight;")
    wait_for("visibleCards().every(card=>card.querySelector('img')?.naturalWidth>0)")
    test('Scrolling loads the next cached images while metadata stays pending',"check(bridge.pendingMetadata.length===2,'metadata still held');check(thumbnails()>3,'later images painted');check(bridge.errors.length===0,bridge.errors.join('; '));")
    snap('thumbnail-replay-cache-ready')
    js("bridge.releaseMetadata=true;bridge.pendingMetadata.splice(0).forEach(resolve=>resolve())")
    js("bridge.holdFile='cached-0.mkv';bridge.heldThumbnails=[];bridge.thumbnailBytes=artworkFixture(320,180,'Old image','image/jpeg');bridge.clips[0].bytes++;click('[data-action=reload-clips]');")
    wait_for("bridge.heldThumbnails.length===1")
    js("bridge.thumbnailBytes=artworkFixture(320,180,'New image','image/jpeg');bridge.clips[0].bytes++;click('[data-action=reload-clips]');")
    wait_for("bridge.heldThumbnails.length===2")
    js("bridge.heldThumbnails[1]()")
    wait_for("q('#clip-video').poster.startsWith('data:')")
    test('The new file revision supplies the current player poster',"window.currentPoster=q('#clip-video').poster;bridge.heldThumbnails[0]();")
    pump(200)
    test('A late result for an older file revision cannot overwrite the player',"check(q('#clip-video').poster===currentPoster,'stale thumbnail ignored');check(bridge.errors.length===0,bridge.errors.join('; '));")
    (ARTIFACTS/'thumbnail-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    (ARTIFACTS/'thumbnail-entry-timing.json').write_text(json.dumps({'overview':overview_timing,'replay':json.loads(js('return JSON.stringify(bridge.previewEntry)'))},indent=2)+'\n')
    print(f'PASS: {len(checks)} thumbnail UI checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
