"""Retained text and control-state checks against the built production UI.

Uses synthetic native responses and a private X display. No hardware/audio capture
or owner state is touched. Build the frontend before running this fixture.
"""
import os
if os.environ.get('REDUNAR_PRIVATE_DESKTOP_FIXTURE') != '1':
    raise SystemExit('Use the private desktop fixture under Xvfb')
os.environ['GDK_BACKEND']='x11'
from functools import partial
import webview_fixture as harness
from webview_fixture import *

ARTIFACTS=NATIVE.parents[1]/'.redunar-build/reports/regular-usage'
ARTIFACTS.mkdir(parents=True,exist_ok=True)
harness.ARTIFACTS=ARTIFACTS
baseline=os.environ.get('REDUNAR_REGULAR_USAGE_BASELINE')=='1'
if os.environ.get('REDUNAR_REGULAR_USAGE_UI_DIST'):
    server.RequestHandlerClass=partial(Handler,directory=os.environ['REDUNAR_REGULAR_USAGE_UI_DIST'])
extra=r"""
bridge.idle=true;bridge.replayInactive=true;bridge.automaticUpdates=false;bridge.startupUpdateAvailable=false;bridge.microphone='default';
bridge.levels={peak:.5,rms:.25,limiting:false};
const regularInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='microphone_levels'){bridge.calls.push({command,args});return structuredClone(bridge.levels);}
 const result=await regularInvoke(command,args);
 if(command==='replay_runtime_status'&&!bridge.idle)result.microphone_state='active';
 return result;
};
window.observeStable=()=>{
 window.mutationCount=0;window.textReplacements=0;
 window.observer=new MutationObserver(records=>{mutationCount+=records.length;textReplacements+=records.filter(x=>x.type==='childList'||x.type==='characterData').length;});
 observer.observe(q('#workspace'),{attributes:true,subtree:true,childList:true,characterData:true});
 window.pollCount=bridge.calls.filter(x=>x.command==='daemon_status').length;
};
window.stableResult=()=>({mutations:mutationCount,textReplacements,polls:bridge.calls.filter(x=>x.command==='daemon_status').length-pollCount});
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
results={}
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#settings');pump(900)
    js('observeStable()');pump(3300);results['settings']=json.loads(js('return JSON.stringify(stableResult())'));js('observer.disconnect()')
    route('global');pump(300)
    js('observeStable()');pump(3300);results['overlay']=json.loads(js('return JSON.stringify(stableResult())'));js('observer.disconnect();click("[data-global-tab=replay]")');pump(700)
    js('''window.testText=q('[data-microphone-test]').firstChild;window.inputValue=q('[data-microphone-source]').nextElementSibling.querySelector('.precision-select-value').firstChild;window.peakText=q('[data-microphone-peak]').firstChild;q('#microphone-gain').focus();window.gainNode=q('#microphone-gain');window.focusNode=document.activeElement;observeStable();''');pump(3300)
    results['idle_microphone']=json.loads(js('return JSON.stringify({...stableResult(),testTextRetained:q("[data-microphone-test]").firstChild===testText,inputTextRetained:q("[data-microphone-source]").nextElementSibling.querySelector(".precision-select-value").firstChild===inputValue,peakTextRetained:q("[data-microphone-peak]").firstChild===peakText,focusRetained:document.activeElement===focusNode})'))
    js('observer.disconnect();bridge.idle=false;bridge.replayInactive=false')
    wait_for("q('[role=meter]').getAttribute('aria-valuenow')==='50'",7000);pump(400)
    js('observeStable();window.testText=q("[data-microphone-test]").firstChild;window.peakText=q("[data-microphone-peak]").firstChild;window.rootScroll=q(".window-content");window.scrollEnd=performance.now()+2400;function scrollTest(now){rootScroll.scrollTop=(Math.sin(now/500)+1)*(rootScroll.scrollHeight-rootScroll.clientHeight)/2;if(now<scrollEnd)requestAnimationFrame(scrollTest);}requestAnimationFrame(scrollTest)');pump(2600)
    results['live_microphone']=json.loads(js('return JSON.stringify({...stableResult(),meterCalls:bridge.calls.filter(x=>x.command==="microphone_levels").length,testTextRetained:q("[data-microphone-test]").firstChild===testText,peakTextRetained:q("[data-microphone-peak]").firstChild===peakText,gainLocked:q("#microphone-gain").disabled})'))
    js('observer.disconnect();bridge.levels={peak:.8,rms:.4,limiting:true}')
    wait_for("q('[role=meter]').getAttribute('aria-valuenow')==='80'")
    test('Changed live measurements and protection status still update',"check(q('[data-microphone-peak]').textContent==='Peak protection','changed level label');check(q('[role=meter]').getAttribute('aria-valuetext').includes('peak protection active'),'accessibility changed');check(q('#microphone-gain').disabled,'live gain remains locked');")
    js('bridge.idle=true;bridge.replayInactive=true')
    wait_for("!q('#microphone-gain').disabled",7000)
    test('Session exit unlocks the retained controls',"check(q('#microphone-gain')===gainNode,'same input');check(q('[role=meter]').getAttribute('aria-valuetext')==='No measurement','inactive meter');check(!bridge.calls.some(x=>['set_microphone_gain','set_replay_microphone','save_global_settings'].includes(x.command)),'no implicit write');check(bridge.errors.length===0,bridge.errors.join('; '));")
    if not baseline:
        for name,record in results.items():
            assert record['polls']>0,(name,'poll never ran')
            assert record['mutations']==0,(name,record)
            assert record['textReplacements']==0,(name,record)
            for field in ['testTextRetained','inputTextRetained','peakTextRetained','focusRetained','gainLocked']:
                if field in record:assert record[field],(name,field)
        checks.append('Unchanged settings, overlay and microphone controls retain text/state through actual polling and live-meter scrolling')
    snap('regular-usage-controls')
    result={'baseline':baseline,'scope':'Built production UI with synthetic native data and actual status/meter polling. Scripted vertical scrolling is not physical presentation FPS.','phases':results,'checks':checks}
    (ARTIFACTS/('baseline.json' if baseline else 'checks.json')).write_text(json.dumps(result,indent=2)+chr(10))
    print(json.dumps(result,indent=2))
finally:
    window.destroy();server.shutdown()
