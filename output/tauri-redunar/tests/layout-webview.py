"""Production layout boundaries with long histories and populated scroll areas."""
from webview_fixture import *

ARTIFACTS = NATIVE.parents[1] / '.redunar-build/reports/layout-ui'
ARTIFACTS.mkdir(parents=True, exist_ok=True)
extra = r"""
bridge.startupUpdateAvailable=false;
bridge.historyFixture=Array.from({length:18},(_,n)=>({id:n+1,game:'Long session fixture',started_unix:1789300000-n*86400,duration_seconds:8640,average_fps:67,timeline:Array.from({length:1800},(_,i)=>({elapsed_seconds:i*8640/1799,fps:i%37<4?0:110+Math.sin(i/12)*20,frame_time_ms:i%37<4?100:9+Math.sin(i/12),cpu_temperature_celsius:60+i%8,gpu_temperature_celsius:70+i%8,cpu_utilization_percent:i%100,gpu_utilization_percent:(i*3)%100}))}));
bridge.clips=Array.from({length:18},(_,i)=>({file_name:`redunar-replay-long-capture-${i}.mkv`,title:`Capture ${i+1}`,game_name:'Local fixture',bytes:68000000,modified_unix_ns:String(1789300000000000000n-BigInt(i)*1000000000n),duration_seconds:30}));
games.push(...Array.from({length:16},(_,i)=>({...games[1],id:String(i+3),name:`Installed fixture ${i+3}`})));
window.layoutBounds=()=>{
 const owner=q('.window-content');
 check(owner.scrollWidth<=owner.clientWidth,'horizontal page overflow');
 check(!q('#workspace h1'),'page titles stay in navigation');
 check(document.fonts.check('14px Space')&&document.fonts.check('12px Plex'),'bundled workspace fonts loaded');
 const sidebar=q('.sidebar').getBoundingClientRect(),header=q('.window-titlebar').getBoundingClientRect();
 check(sidebar.top===header.bottom&&sidebar.bottom<=innerHeight,'native sidebar bounds');
};
window.dialogBounds=(mustFit=true)=>{
 const dialog=q('#dialog'),style=getComputedStyle(dialog),footer=dialog.querySelector('.dialog-actions');
 const box=dialog.getBoundingClientRect(),actions=footer.getBoundingClientRect();
 check(dialog.open,'dialog open');
 check(Math.abs(parseFloat(style.paddingLeft)-parseFloat(style.getPropertyValue('--dialog-padding')))<.1,'footer and shell share padding');
 check(dialog.scrollWidth<=dialog.clientWidth,'no horizontal dialog overflow');
 if(mustFit)check(dialog.scrollHeight<=dialog.clientHeight,'short dialog needs no vertical scrolling');
 check(actions.left>=box.left+.5&&actions.right<=box.right-.5,'footer stays inside shell');
 check(style.boxShadow==='none','shadowless shared dialog');
 check(getComputedStyle(dialog.querySelector('.dialog-heading h2')).fontSize==='18px','shared dialog heading');
 check(getComputedStyle(footer).paddingTop==='20px','shared footer spacing');
 check(!dialog.querySelector('.dialog-close'),'no redundant close cross');
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))

def capture(name):
    loop = GLib.MainLoop()
    def done(w, value, _):
        w.get_snapshot_finish(value).write_to_png(str(ARTIFACTS / (name + '.png')))
        loop.quit()
    view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, done, None)
    loop.run()

try:
    window.resize(2560,1440)
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#history')
    pump(1400)
    for width,height in [(2560,1440),(1920,1080),(1440,900),(1040,800),(640,720)]:
        window.resize(width,height);pump(400)
        test(f'History plot stays inside its frame at {width}px', """
          layoutBounds();
          const chart=q('.history-timeline-chart'),svg=chart.querySelector('svg'),frame=chart.getBoundingClientRect(),plot=svg.getBoundingClientRect(),axis=q('.history-time-axis').getBoundingClientRect();
          check(plot.top>=frame.top&&plot.bottom<=frame.bottom,'SVG fits plot frame');
          check(axis.top>=frame.bottom,'time labels follow plot frame');
          const point=new DOMPoint(0,210).matrixTransform(svg.getScreenCTM());
          const zero=q('.y-axis [data-axis-value="0"]').getBoundingClientRect();
          check(point.y<frame.bottom&&Math.abs(point.y-zero.top-zero.height/2)<1,'zero FPS stays inside plot and aligns with label');
          check([...svg.querySelectorAll('polyline.timeline-trace')].reduce((n,line)=>n+line.points.numberOfItems,0)===1800,'all recorded observations retained');
        """)
    window.resize(2560,1440);pump(400)
    test('History reserves a scrollbar channel beside session borders', "const list=q('#session-list'),row=list.querySelector('.history-row'),viewport=list.getBoundingClientRect(),card=row.getBoundingClientRect();check(list.scrollHeight>list.clientHeight,'populated session rail');check(viewport.right-card.right>=21.9,'room for overlay scrollbar and hover movement');list.scrollTop=list.scrollHeight;check(list.scrollTop>0,'native list scrolling');")
    capture('history-wide-populated')
    js("q('.history-timeline-chart').scrollIntoView({block:'start'})")
    capture('history-wide-plot')
    for metric in ['frame-time','temperature','utilization','fps']:
        js("click('[data-history-view="+metric+"]')");pump(650)
        test('History '+metric+' keeps its plot and ticks inside the frame', "const chart=q('.history-timeline-chart').getBoundingClientRect(),svg=q('.history-timeline-chart svg').getBoundingClientRect();check(svg.top>=chart.top&&svg.bottom<=chart.bottom,'metric SVG bounds');check(qa('.y-axis span').every(label=>{const b=label.getBoundingClientRect();return b.top>=chart.top&&b.bottom<=chart.bottom;}),'tick label bounds');")
    for width,height in [(2560,1440),(1280,900),(640,800)]:
        window.resize(width,height);pump(350)
        for page in ['overview','library','global','replay','settings','history']:
            route(page);pump(300)
            test(f'{page} keeps shared shell and page bounds at {width}px', 'layoutBounds();')
            if width==1280:capture(page+'-populated')
    window.resize(1440,1000);pump(400);route('library')
    test('Library reserves the same scrollbar clearance for game cards', "const list=q('#library-list'),row=list.querySelector('.library-game');check(list.scrollHeight>list.clientHeight,'populated game rail');check(list.getBoundingClientRect().right-row.getBoundingClientRect().right>=21.9,'game card clear of overlay scrollbar');")
    route('replay')
    test('The video meets the rounded editor header with square top corners', "const stage=getComputedStyle(q('.native-video-stage'));check(stage.borderTopLeftRadius==='0px'&&stage.borderTopRightRadius==='0px','square video corners');check(parseFloat(getComputedStyle(q('.editor')).borderTopLeftRadius)>0,'editor shell stays rounded');const list=q('.clip-list'),row=list.querySelector('.clip-card');check(list.scrollHeight>list.clientHeight,'populated clip rail');check(list.getBoundingClientRect().right-row.getBoundingClientRect().right>=21.9,'clip card clear of overlay scrollbar');")
    capture('replay-square-video')
    for action,label in [('delete-selected-clip','trash'),('edit-clip-filename','filename'),('compress-selected-clip','compression')]:
        js("click('[data-action="+action+"]')");pump(350)
        test(label+' dialog fits without either scrollbar', 'dialogBounds();')
        if label=='compression':
            test('Compression uses the same shell and footer as other dialogs', "const dialog=q('#dialog'),footer=dialog.querySelector('.dialog-actions');check(Math.abs(dialog.getBoundingClientRect().width-440)<1,'established compression width');check(getComputedStyle(dialog).borderTopLeftRadius==='18px','shared shell radius');check(parseFloat(getComputedStyle(footer).paddingTop)>0&&getComputedStyle(footer).borderTopWidth==='1px','shared inset footer');")
        capture(label+'-dialog')
        window.resize(360,320);pump(350)
        test(label+' dialog has only vertical overflow in a short window', 'dialogBounds(false);')
        js("q('#dialog-error').hidden=false;q('#dialog-error').textContent='Cannot save '+('f'.repeat(240))+'.mkv';")
        test(label+' dialog wraps long failure details without horizontal scrolling', 'dialogBounds(false);')
        capture(label+'-dialog-short')
        js("click('#dialog [data-close]')")
        window.resize(1440,1000);pump(350)
    test('Layout checks leave no uncaught application error', "check(bridge.errors.length===0,bridge.errors.join('; '));")
    (ARTIFACTS/'layout-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} layout checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
