"""History metric morphs with exact inspection and bounded cleanup in WebKit."""
from webview_fixture import *

gtk_settings=Gtk.Settings.get_default()
original_animations=gtk_settings.get_property('gtk-enable-animations')
gtk_settings.set_property('gtk-enable-animations',True)
extra=r"""
bridge.idle=true;bridge.automaticUpdates=false;bridge.historyFrames=0;
const frame=window.requestAnimationFrame.bind(window);
window.requestAnimationFrame=callback=>{bridge.historyFrames++;return frame(callback);};
window.graph=()=>q('.history-timeline-chart svg');
window.trace=()=>q('polyline.timeline-trace').getAttribute('points');
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#history');pump(500)
    wait_for("q('polyline.timeline-trace')&&matchMedia('(prefers-reduced-motion: no-preference)').matches")
    test('History entry shows exact data with no graph clock',"check(!graph().dataset.morphing,'entry stationary');window.fps=trace();window.catalog=q('.history-catalog');window.search=q('#history-search');window.reads=bridge.calls.filter(c=>c.command==='session_history').length;window.button=q('[data-history-view=frame-time]');button.focus();button.click();check(graph().dataset.morphing==='true','metric morph started');check(trace().split(/[ ,]/).every((value,i)=>Math.abs(Number(value)-Number(fps.split(/[ ,]/)[i]))<.001),'starts at previous geometry');check(document.activeElement===button,'metric focus retained');check(q('.history-catalog')===catalog&&q('#history-search')===search,'surrounding view retained');")
    pump(130)
    test('Graph interpolates only after a metric selection',"check(trace()!==fps,'visible intermediate shape');check(graph().dataset.morphing==='true','bounded morph active');window.frames=bridge.historyFrames;button.click();check(bridge.historyFrames===frames,'active metric does not restart');")
    snap('history-motion-transition')
    js("click('[data-history-view=temperature]');click('[data-history-view=utilization]');click('[data-history-view=fps]')")
    pump(600)
    test('Rapid changes finish at the exact selected graph',"check(trace()===fps,'exact original spikes and timestamps');check(!graph().dataset.morphing,'clock released');check(q('[data-history-view=fps]').getAttribute('aria-pressed')==='true','selected metric correct');window.frames=bridge.historyFrames;")
    pump(2300)
    test('Polling does not restart graph animation or reload history',"check(bridge.historyFrames===frames,'no idle frames');check(bridge.calls.filter(c=>c.command==='session_history').length===reads,'no history reads');click('[data-history-view=temperature]');check(graph().dataset.morphing==='true','morph active');check(Number(q('polyline.gpu').getAttribute('opacity'))===0,'new GPU fades from hidden');check(q('polygon.history-motion-exit'),'departing area retained for fade');const slider=q('[data-history-cursor]');slider.value='1200';slider.dispatchEvent(new Event('input',{bubbles:true}));check(!graph().dataset.morphing,'inspection settles immediately');check(q('#history-moment-cpu-temp').textContent==='61.0°C','exact CPU reading');window.temperature=q('polyline.cpu').getAttribute('points');check(q('polyline.cpu').points[0].y===q('polyline.cpu').points[95].y,'exact constant fixture geometry');")
    snap('history-motion-ready')
    js("click('[data-history-view=fps]')");pump(120)
    test('Secondary trace and fill fade while the graph morphs',"const secondary=q('polyline.gpu.history-motion-exit');check(secondary&&Number(secondary.getAttribute('opacity'))>0&&Number(secondary.getAttribute('opacity'))<1,'secondary fades out');const area=q('polygon.timeline-area');check(Number(area.getAttribute('opacity'))>0&&Number(area.getAttribute('opacity'))<1,'fill fades in');")
    snap('history-motion-secondary-fade')
    pump(600)
    test('Completed fades remove temporary shapes and opacity',"check(!q('.history-motion-exit'),'no exit shapes');check(!q('polyline.primary').hasAttribute('opacity'),'opacity released');check(!q('polygon.timeline-area').hasAttribute('opacity'),'fill opacity released');click('[data-history-view=temperature]');")
    pump(600)
    test('Scrolling settles motion immediately',"click('[data-history-view=fps]');check(graph().dataset.morphing==='true','active before scroll');q('.window-content').dispatchEvent(new Event('scroll'));check(!graph().dataset.morphing&&trace()===fps,'exact geometry after scroll');window.oldGraph=graph();click('[data-history-view=temperature]');window.oldGraph=graph();")
    route('global')
    test('Navigation releases the detached graph and queued frames',"check(!oldGraph.isConnected&&!oldGraph.dataset.morphing,'old graph settled');window.frames=bridge.historyFrames;")
    pump(600)
    test('Leaving History leaves no graph work',"check(bridge.historyFrames===frames,'no detached frames');")
    route('history')
    js("click('[data-history-view=fps]')")
    gtk_settings.set_property('gtk-enable-animations',False)
    wait_for("matchMedia('(prefers-reduced-motion: reduce)').matches&&!graph().dataset.morphing")
    test('Reduced motion settles a running graph and keeps switches immediate',"check(trace()===fps,'running graph settled');window.frames=bridge.historyFrames;click('[data-history-view=temperature]');check(!graph().dataset.morphing&&q('polyline.cpu').getAttribute('points')===temperature,'reduced motion exact');check(bridge.historyFrames===frames,'no reduced frames');")
    gtk_settings.set_property('gtk-enable-animations',True)
    wait_for("matchMedia('(prefers-reduced-motion: no-preference)').matches")
    js("bridge.historyFixture=[{id:20,started_unix:1789300000,game:'Gaps and isolated readings',duration_seconds:12,timeline:[0,3,9,12].map((elapsed_seconds,i)=>({elapsed_seconds,fps:60,frame_time_ms:10,cpu_temperature_celsius:i===1?null:70,gpu_temperature_celsius:i===2?80:null}))}];click('[data-action=reload-history]')");pump()
    js("click('[data-history-view=fps]')");pump(600)
    test('Missing observations do not cancel other series or bridge gaps',"click('[data-history-view=temperature]');check(graph().dataset.morphing==='true','partial sensor coverage animates');check(q('circle.gpu'),'isolated observation retained');check(qa('polyline.cpu').length===1,'one continuous valid segment');check(q('polyline.cpu').points.numberOfItems===2,'gap retained');")
    pump(600)
    js("bridge.historyFixture=[{id:22,started_unix:1789300000,game:'Partial sensor coverage',duration_seconds:95,timeline:Array.from({length:96},(_,elapsed_seconds)=>({elapsed_seconds,fps:60+elapsed_seconds%8,frame_time_ms:10+elapsed_seconds%4,cpu_temperature_celsius:elapsed_seconds<3?null:65+elapsed_seconds%5,gpu_temperature_celsius:elapsed_seconds<5||elapsed_seconds===40?null:72+elapsed_seconds%4,cpu_utilization_percent:elapsed_seconds<2?null:30+elapsed_seconds%9,gpu_utilization_percent:elapsed_seconds<4?null:70+elapsed_seconds%6}))}];click('[data-action=reload-history]')");pump()
    # Exercise every direction with unequal frame/CPU/GPU coverage. Isolated
    # samples remain visible, and a GPU gap cannot disable the CPU transition.
    for source in ['fps','frame-time','temperature','utilization']:
        for target in ['fps','frame-time','temperature','utilization']:
            if source==target:continue
            js(f"click('[data-history-view={source}]')");pump(600)
            test(f'Partial coverage animates {source} to {target}',f"click('[data-history-view={target}]');check(graph().dataset.morphing==='true','directed metric transition starts');window.transitionStart=trace();")
            pump(120)
            test(f'Partial coverage progresses {source} to {target}',"check(graph().dataset.morphing==='true','transition stays active');check(trace()!==transitionStart||qa('polyline.timeline-trace').some(line=>Number(line.getAttribute('opacity'))>0&&Number(line.getAttribute('opacity'))<1),'visible shape or fade progress');")
            if source=='frame-time' and target=='temperature':snap('history-motion-partial-temperature')
            pump(600)
            test(f'Partial coverage settles {source} to {target}',"check(!graph().dataset.morphing&&!q('.history-motion-exit'),'cleanup');check(qa('polyline.timeline-trace').every(line=>!line.hasAttribute('opacity')),'exact final opacity');")
    js("bridge.historyFixture=[{id:21,started_unix:1789300000,game:'Dense history',duration_seconds:7200,timeline:Array.from({length:7200},(_,elapsed_seconds)=>({elapsed_seconds,fps:elapsed_seconds===3721?10:60,frame_time_ms:10,cpu_temperature_celsius:elapsed_seconds===6000?95:70,gpu_temperature_celsius:75}))}];click('[data-action=reload-history]')");pump()
    js("click('[data-history-view=fps]')");pump(600)
    test('Dense histories morph within the per-frame point budget',"window.frames=bridge.historyFrames;window.denseStart=trace();click('[data-history-view=temperature]');check(graph().dataset.morphing==='true','dense graph morphs');check(qa('polyline.timeline-trace').reduce((sum,line)=>sum+line.points.numberOfItems,0)<=1024,'bounded geometry');check(bridge.historyFrames>frames,'bounded clock active');")
    pump(140)
    test('Dense morph visibly changes shape before completing',"check(graph().dataset.morphing==='true','transition still active');check(q('polyline.cpu').points[0].y!==Number(denseStart.split(/[ ,]/)[1]),'shape moved from previous metric');")
    snap('history-motion-dense')
    pump(600)
    test('Dense completion restores every exact observation and stops work',"check(!graph().dataset.morphing&&!q('.history-motion-exit'),'settled and cleaned');check(q('polyline.cpu').points.numberOfItems===7200&&q('polyline.gpu').points.numberOfItems===7200,'all points restored');const points=q('polyline.cpu').points;check(points[6000].y<points[5999].y,'recorded spike retained');window.frames=bridge.historyFrames;")
    pump(600)
    test('Dense graphs leave no idle work or errors',"check(bridge.historyFrames===frames,'no idle work');check(bridge.errors.length===0,bridge.errors.join('; '));")
    (ARTIFACTS/'history-motion-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} History motion checks. Screenshots: {ARTIFACTS}')
finally:
    gtk_settings.set_property('gtk-enable-animations',original_animations)
    window.destroy();server.shutdown()
