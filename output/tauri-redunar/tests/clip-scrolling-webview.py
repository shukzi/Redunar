"""Warm Replay scrolling must not synchronously measure every mounted card."""
from webview_fixture import *

extra=r"""
bridge.idle=true;bridge.automaticUpdates=false;bridge.startupUpdateAvailable=false;
window.scrollProbe={reads:0,enabled:false};
const originalRect=Element.prototype.getBoundingClientRect;
Element.prototype.getBoundingClientRect=function(){
 if(scrollProbe.enabled&&this.matches('[data-clip]'))scrollProbe.reads++;
 return originalRect.call(this);
};
window.runRailScroll=()=>{
 window.scrollFinished=false;window.frameIntervals=[];
 let frame=0,previous=performance.now();const rail=q('.clip-list');
 function step(now){
  frameIntervals.push(now-previous);previous=now;
  rail.scrollTop=(frame%30)*40;
  if(++frame<90)requestAnimationFrame(step);else scrollFinished=true;
 }
 requestAnimationFrame(step);
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
measurements=[]
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#replay')
    wait_for("window.q&&q('.clip-list')")
    for count in [20,200,1000]:
        js(f"bridge.clips=Array.from({{length:{count}}},(_,i)=>({{file_name:'scroll-'+i+'.mkv',bytes:7800000,modified_unix_ns:String(i+1),duration_seconds:29,game_name:'Private scroll fixture'}}));click('[data-action=reload-clips]');")
        wait_for(f"qa('[data-clip]').length==={count}")
        pump(300)
        js('runRailScroll()')
        wait_for('scrollFinished',timeout_ms=15000)
        pump(400)
        js("window.warmReads=bridge.calls.filter(c=>c.command==='clip_thumbnail'||c.command==='clip_metadata').length;window.retainedPlayer=q('#clip-video');window.retainedRail=q('.clip-list');scrollProbe.reads=0;scrollProbe.enabled=true;runRailScroll();")
        wait_for('scrollFinished',timeout_ms=15000)
        pump(200)
        test(f'Warm scrolling with {count} clips does not scan card geometry or reload cached previews', r"""
            scrollProbe.enabled=false;
            check(retainedRail.scrollTop>0&&retainedRail.scrollHeight>retainedRail.clientHeight,'the bounded rail actually scrolled');
            check(qa('.thumbnail img').some(image=>image.naturalWidth>0),'native preview fixture painted images');
            check(scrollProbe.reads===0,'synchronous card bounds reads: '+scrollProbe.reads);
            check(bridge.calls.filter(c=>c.command==='clip_thumbnail'||c.command==='clip_metadata').length===warmReads,'warm cards issue no new preview requests');
            check(q('.clip-list')===retainedRail&&q('#clip-video')===retainedPlayer,'scroll retains rail and player');
            check(bridge.errors.length===0,bridge.errors.join('; '));
        """)
        measurements.append({'clips':count,'card_bounds_reads':0,'frame_intervals_ms':json.loads(js('return JSON.stringify(frameIntervals)'))})
    js("""
        window.edgeCard=qa('[data-clip]')[40];window.edgeFile=edgeCard.dataset.clip;
        const list=q('.clip-list'),bottom=Math.min(list.getBoundingClientRect().bottom,innerHeight);
        list.scrollTop+=edgeCard.getBoundingClientRect().top-bottom;
    """)
    pump(300)
    test('A card touching the viewport edge is prefetched before scrolling further inside',r"""
        check(bridge.calls.some(c=>c.command==='clip_thumbnail'&&c.args.fileName===edgeFile),'edge preview request was not skipped');
        q('.clip-list').scrollTop+=40;
    """)
    wait_for("edgeCard.querySelector('img')?.naturalWidth>0")
    window.resize(640,800);pump(600)
    js("q('.clip-list').scrollIntoView({block:'start'});window.responsiveReads=bridge.calls.filter(c=>c.command==='clip_thumbnail'||c.command==='clip_metadata').length;")
    pump(700)
    test('Responsive rail changes load only exposed previews instead of the whole inventory',r"""
        check(bridge.calls.filter(c=>c.command==='clip_thumbnail'||c.command==='clip_metadata').length-responsiveReads<=20,'responsive mode does not queue the entire catalog');
        const list=q('.clip-list');check(list.scrollWidth<=list.clientWidth+1,'narrow rail fits');
    """)
    route('library');pump(200)
    test('Leaving Replay cancels visible-card observation without background preview requests',r"""
        window.leftReads=bridge.calls.filter(c=>c.command==='clip_thumbnail'||c.command==='clip_metadata').length;
    """)
    pump(500)
    test('No queued off-page preview work resumes after navigation',r"""
        check(bridge.calls.filter(c=>c.command==='clip_thumbnail'||c.command==='clip_metadata').length===leftReads,'no off-page preview work');
    """)
    (ARTIFACTS/'clip-scrolling-timing.json').write_text(json.dumps(measurements,indent=2)+'\n')
    print(f'PASS: {len(checks)} clip scrolling checks; 20/200/1000 warm clips, zero synchronous card-bound reads.')
finally:
    window.destroy();server.shutdown()
