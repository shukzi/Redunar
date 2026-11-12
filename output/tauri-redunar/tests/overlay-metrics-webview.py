"""Expanded overlay settings in the production UI with isolated native responses."""
import webview_fixture as harness
from webview_fixture import *
ARTIFACTS=NATIVE.parents[1]/'.redunar-build/reports/overlay-metrics'
ARTIFACTS.mkdir(parents=True,exist_ok=True)
harness.ARTIFACTS=ARTIFACTS
extra="bridge.idle=true;bridge.startupUpdateAvailable=false;bridge.automaticUpdates=false;"
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#global');pump(1200)
    test('Default remains Compact',"check(q('button[data-preset=Compact]').getAttribute('aria-pressed')==='true','Compact selected');check(qa('.hud-telemetry-row').length===6,'six default metrics');")
    snap('default')
    js("click('[data-overlay-editor-tab=metrics]');click('button[data-preset=Detailed]')");pump(300)
    test('Detailed selects the eight requested metrics without a graph',"check(qa('[data-metric]').length===17,'seventeen choices');check(qa('[data-metric]:checked').length===8,'Detailed selection');check(qa('.hud-telemetry-row').length===8,'Detailed preview rows');check(!q('#hud canvas')&&!q('#hud svg'),'no graph');check(!q('.settings-save-bar').hidden,'draft actions');check(q('[data-metric=RAM]').closest('.metric-group').getAttribute('aria-label')==='Hardware metrics','RAM belongs to hardware');")
    test('Metric groups use balanced columns',"const rows=qa('[aria-label=\"Hardware metrics\"] .metric-toggle-row');check(rows.length===12,'all hardware choices');check(Math.abs(rows[0].getBoundingClientRect().top-rows[1].getBoundingClientRect().top)<2,'CPU and GPU align');check(rows[1].getBoundingClientRect().left>rows[0].getBoundingClientRect().right,'two columns');")
    js("q('[aria-label=\"Hardware metrics\"]').scrollIntoView({block:'center'})");pump(200)
    snap('detailed')
    js("click('button[data-preset=Custom]');q('[data-metric=\"CPU power\"]').click()");pump(200)
    test('Custom selection and concise labels work',"check(qa('[data-metric]:checked').length===7,'one deselected');check(qa('.metric-description').length===1,'only API has a description');check(q('[data-metric=CPU]').getAttribute('aria-label')==='CPU load','CPU load label');check(q('[data-metric=GPU]').getAttribute('aria-label')==='GPU load','GPU load label');check(q('[data-metric=\"Graphics API\"]').closest('label').textContent.includes('DirectX'),'API explanation');click('[data-action=discard-global]');check(qa('.hud-telemetry-row').length===6,'discard restores Compact');check(q('.settings-save-bar').hidden,'clean after discard');")
    js("click('button[data-preset=Detailed]');click('[data-action=save-global]')");pump(400)
    test('Saved Detailed retains the requested selection',"const saved=bridge.calls.findLast(c=>c.command==='save_global_settings').args.input;check(saved.preset==='Detailed','Detailed saved');check(qa('[data-metric]:checked').length===8,'saved Detailed selection');check(saved.metrics.join(',')==='FPS','remembered custom choice retained');check(q('.settings-save-bar').hidden,'clean saved state');click('[data-overlay-editor-tab=appearance]');click('[data-layout=Ribbon]');")
    pump(200);snap('ribbon')
    test('Ribbon preview fits the preview area',"const h=q('#hud').getBoundingClientRect(),s=q('.overlay-scene').getBoundingClientRect();check(h.right<=s.right+1&&h.left>=s.left-1,'ribbon fits');click('[data-action=discard-global]');")
    js("q('[data-global=position]').value='Bottom right';q('[data-global=position]').dispatchEvent(new Event('change',{bubbles:true}))");pump(200)
    test('Position selector retains expanded selection',"check(q('#hud').dataset.position==='Bottom right','position changed');check(qa('.hud-telemetry-row').length===8,'metrics retained');click('[data-action=discard-global]');")
    js("click('[data-global-tab=replay]')");pump(200);snap('replay-reference')
    test('No runtime script errors',"check(bridge.errors.length===0,bridge.errors.join('; '));")
    (ARTIFACTS/'checks.json').write_text(json.dumps(checks,indent=2))
    print(json.dumps(checks,indent=2))
finally:
    window.destroy();server.shutdown()
