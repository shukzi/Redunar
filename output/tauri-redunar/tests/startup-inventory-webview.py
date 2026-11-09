"""Initial inventory reuse must retain failed-read and save-during-read recovery."""
from webview_fixture import *

extra = r"""
bridge.idle=true;bridge.startupUpdateAvailable=false;
bridge.startupCase=new URLSearchParams(location.search).get('case');
bridge.completedRevision=0;bridge.historyRevision=0;
bridge.initialReads={};
const originalInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 const value=await originalInvoke(command,args);
 const first=(bridge.initialReads[command]=(bridge.initialReads[command]||0)+1)===1;
 if(bridge.startupCase==='failed'&&first&&['replay_clips','session_history'].includes(command))throw Error('Fixture initial read failed');
 if(first&&((bridge.startupCase==='clip-race'&&command==='replay_clips')||(bridge.startupCase==='history-race'&&command==='session_history'))){
  const snapshot=structuredClone(value);
  await new Promise(resolve=>bridge.releaseInitial=resolve);
  return snapshot;
 }
 if(command==='session_status')return {...value,phase:bridge.ended?'Ended':value.phase,history_revision:bridge.historyRevision};
 if(command==='replay_runtime_status')return {...value,completed_save_revision:bridge.completedRevision};
 return value;
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/?case=zero#overview')
    wait_for("window.bridge&&bridge.startupCase==='zero'&&bridge.calls.some(c=>c.command==='check_for_updates')&&q('.recent-capture-grid a')")
    test('Overview loads clips without fetching saved session timelines', """
      for(const command of ['replay_clips','replay_storage_status'])
        check(bridge.calls.filter(c=>c.command===command).length===1,command+' read once');
      check(!bridge.calls.some(c=>c.command==='session_history'),'history not requested');
      for(const command of ['daemon_status','replay_runtime_status','session_status','module_statuses','shortcut_status'])
        check(bridge.calls.filter(c=>c.command===command).length===1,command+' starts its normal cadence without an immediate duplicate');
    """)
    js("window.polls=bridge.calls.filter(c=>c.command==='daemon_status').length")
    wait_for("bridge.calls.filter(c=>c.command==='daemon_status').length>polls")
    test('Idle Overview leaves History unloaded', "check(bridge.calls.filter(c=>c.command==='replay_clips').length===1,'clips not reread');check(!bridge.calls.some(c=>c.command==='session_history'),'no history reads')")
    route('history')
    wait_for("q('#session-list [data-session]')")
    test('First History visit loads the saved sessions once', "check(bridge.calls.filter(c=>c.command==='session_history').length===1,'one history read')")
    route('overview')
    js("bridge.historyFixture=[{id:77,game:'Just completed',started_unix:1792250000,duration_seconds:10,timeline:[]}];bridge.historyRevision=1;bridge.ended=true;window.polls=bridge.calls.filter(c=>c.command==='daemon_status').length")
    wait_for("bridge.calls.filter(c=>c.command==='daemon_status').length>polls")
    test('A completed session does not load its history on Overview', "check(bridge.calls.filter(c=>c.command==='session_history').length===1,'no off-page history refresh')")
    route('history')
    wait_for("q('#session-list')?.textContent.includes('Just completed')")
    test('History sees the revision of a session hidden from Overview', "check(bridge.calls.filter(c=>c.command==='session_history').length===2,'updated history read once')")

    view.load_uri(f'http://127.0.0.1:{server.server_port}/?case=failed#overview')
    wait_for("window.bridge&&bridge.startupCase==='failed'&&bridge.calls.some(c=>c.command==='check_for_updates')&&q('.recent-capture-grid a')")
    test('Failed initial clip reads retry without loading History', "check(bridge.calls.filter(c=>c.command==='replay_clips').length===2,'clips recovered');check(!bridge.calls.some(c=>c.command==='session_history'),'history deferred')")
    route('history')
    wait_for("q('#workspace').textContent.includes('Fixture initial read failed')")
    wait_for("q('#session-list [data-session]')")
    test('A failed first History read shows the error and retries', "check(bridge.calls.filter(c=>c.command==='session_history').length===2,'history recovered')")

    view.load_uri(f'http://127.0.0.1:{server.server_port}/?case=clip-race#overview')
    wait_for("window.bridge&&bridge.startupCase==='clip-race'&&bridge.releaseInitial")
    js("bridge.clips.unshift({file_name:'new-startup-save.mkv',bytes:123456,modified_unix_ns:'1792250000000000000'});bridge.completedRevision=1;bridge.releaseInitial()")
    wait_for("bridge.calls.some(c=>c.command==='check_for_updates')&&q('.recent-capture-grid')?.textContent.includes('new-startup-save')")
    test('A save during the initial inventory read is not acknowledged early', "check(bridge.calls.filter(c=>c.command==='replay_clips').length===2,'new revision reread');check(q('.recent-capture-grid').textContent.includes('new-startup-save'),'new recording visible')")

    view.load_uri(f'http://127.0.0.1:{server.server_port}/?case=history-race#history')
    wait_for("window.bridge&&bridge.startupCase==='history-race'&&bridge.releaseInitial")
    test('A pending History read does not look like an empty list', "check(q('.history-empty h2').textContent==='Loading sessions…','loading label');check(q('[data-action=reload-history]').disabled,'a second read cannot start');")
    js("bridge.historyFixture=[{id:99,game:'Startup completion',started_unix:1792250000,duration_seconds:10,frame_intervals_ns:[],timeline:[]}];bridge.historyRevision=1;bridge.releaseInitial()")
    wait_for("bridge.calls.some(c=>c.command==='check_for_updates')&&q('#session-list')?.textContent.includes('Startup completion')")
    test('A session completed during initial history loading still refreshes', "check(bridge.calls.filter(c=>c.command==='session_history').length===2,'history reread');check(bridge.errors.length===0,bridge.errors.join('; '))")
    snap('startup-history-refreshed')
    (ARTIFACTS/'startup-inventory-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} startup inventory checks')
finally:
    window.destroy();server.shutdown()
