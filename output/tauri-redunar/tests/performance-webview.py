"""Refresh regressions in production WebKit, with private command responses."""
from webview_fixture import *

extra = r"""
bridge.idle=true;
bridge.holdInputs=true;
const originalInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='check_for_updates'){
  await new Promise(resolve=>bridge.releaseUpdate=resolve);
 }
 if(command==='microphone_inputs'&&bridge.holdInputs){
  bridge.inputRequests=(bridge.inputRequests||0)+1;
  await new Promise(resolve=>bridge.releaseInputs=resolve);
 }
 return originalInvoke(command,args);
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#overview')
    pump(500)
    wait_for("q('#session-name')?.textContent==='No active game'&&bridge.releaseUpdate")
    test('Startup does not enumerate microphone devices', "check(!bridge.inputRequests,'no input discovery');check(q('.recent-capture-grid a'),'clips loaded');")
    js("window.summaryNode=q('#session-capture-summary').firstElementChild;window.summaryMutations=0;window.summaryObserver=new MutationObserver(records=>summaryMutations+=records.length);summaryObserver.observe(q('#session-capture-summary'),{childList:true,subtree:true});window.initialPolls=bridge.calls.filter(c=>c.command==='daemon_status').length;")
    pump(2300)
    test('Unchanged status retains its DOM across real polling', "check(bridge.calls.filter(c=>c.command==='daemon_status').length>initialPolls,'polling ran');check(q('#session-capture-summary').firstElementChild===summaryNode,'same status node');check(summaryMutations===0,'zero status mutations, got '+summaryMutations);bridge.replayFailure=true;")
    wait_for("q('#session-capture-summary').textContent.includes('encoder failed')")
    test('Changed status still updates and exposes failure honestly', "check(q('#session-capture-summary').firstElementChild!==summaryNode,'new failure');check(q('.capture-summary').getAttribute('role')==='alert','failure alert');summaryObserver.disconnect();bridge.replayFailure=false;")
    route('replay')
    js("window.playerNode=q('#clip-video');window.railNode=q('.clip-list');window.playbackCalls=bridge.calls.filter(c=>c.command==='clip_playback_path').length;q('#clip-search').focus();q('#clip-search').value='long';q('#clip-search').dispatchEvent(new Event('input',{bubbles:true}));bridge.releaseUpdate();")
    wait_for("q('.sidebar-update-badge')")
    test('Startup update completion preserves Replay, filter and focus', "check(q('#clip-video')===playerNode,'player retained');check(q('.clip-list')===railNode,'rail retained');check(q('#clip-search').value==='long'&&document.activeElement===q('#clip-search'),'filter and focus retained');check(bridge.calls.filter(c=>c.command==='clip_playback_path').length===playbackCalls,'no repeated preparation');")
    js("q('#toast').hidden=true")
    route('global')
    js("q('[data-global=scale]').value='105';q('[data-global=scale]').dispatchEvent(new Event('input',{bubbles:true}));click('[data-global-tab=replay]');")
    wait_for("bridge.releaseInputs")
    test('Microphone discovery is lazy and visibly pending', "check(bridge.inputRequests===1,'one request');check(q('.microphone-device-status').textContent.includes('Loading'),'pending feedback');check(q('[data-microphone-source]').disabled,'input disabled');window.settingsNode=q('[data-global=quality]');window.inputNode=q('[data-microphone-source]');q('[data-global=quality]').value='High';q('[data-global=quality]').dispatchEvent(new Event('change',{bubbles:true}));")
    js("q('.microphone-device').scrollIntoView({block:'center'})")
    snap('performance-microphone-loading')
    route('overview');route('global')
    test('Navigation shares the outstanding discovery', "check(bridge.inputRequests===1,'request coalesced');window.settingsNode=q('[data-global=quality]');window.inputNode=q('[data-microphone-source]');bridge.holdInputs=false;bridge.releaseInputs();")
    wait_for("q('[data-microphone-source]')?.querySelector('option[value=usb-mic]')")
    test('Discovery completion preserves settings and retained controls', "check(q('[data-global=quality]')===settingsNode&&settingsNode.value==='High','quality node and draft');check(q('[data-microphone-source]')===inputNode,'input node retained');check(!q('.settings-save-bar').hidden,'draft still unsaved');")
    js("click('[data-global-tab=overlay]')")
    test('Discovery completion preserves the appearance draft', "check(q('[data-global=scale]').value==='105','appearance retained');click('[data-global-tab=replay]');bridge.failInputs=true;click('[data-action=reload-microphone-devices]');")
    wait_for("q('.microphone-device-status')?.textContent.includes('Fixture audio server unavailable')")
    snap('performance-microphone-error')
    test('Failed discovery offers retry without replacing unrelated controls', "window.settingsNode=q('[data-global=quality]');check(settingsNode.value==='High','failed read retained draft');check(!q('[data-action=reload-microphone-devices]').disabled,'retry enabled');bridge.failInputs=false;click('[data-action=reload-microphone-devices]');")
    wait_for("!q('.microphone-device-status')")
    test('Retry clears failure and does not save the pending draft', "check(q('[data-global=quality]')===settingsNode,'unrelated settings retained');check(!bridge.calls.some(c=>c.command==='save_global_settings'),'no implicit save');check(bridge.errors.length===0,bridge.errors.join('; '));")
    snap('performance-microphone-ready')
    window.resize(640,720);pump()
    js("q('.microphone-device').scrollIntoView({block:'center'})")
    snap('performance-microphone-narrow')
    (ARTIFACTS/'performance-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} performance checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
