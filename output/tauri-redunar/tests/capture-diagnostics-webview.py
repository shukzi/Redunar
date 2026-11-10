"""Capture diagnostics and visible session failures with isolated native responses."""
import webview_fixture as harness
from webview_fixture import *
ARTIFACTS=NATIVE.parents[1]/'.redunar-build/reports/capture-diagnostics-ui'
ARTIFACTS.mkdir(parents=True,exist_ok=True);harness.ARTIFACTS=ARTIFACTS
extra=r"""
bridge.startupUpdateAvailable=false;bridge.automaticUpdates=false;
bridge.capture={phase:'Armed',received_frame_count:0,dropped_frame_count:0};
bridge.failDiagnostics=new URLSearchParams(location.search).has('diagnostics-unavailable');
bridge.monitorSamples=12;
const now=Date.now;
Date.now=()=>now()+(bridge.clockOffset||0);
const invoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='session_status'){
  bridge.calls.push({command,args});
  if(bridge.failSession)throw Error('Fixture session unavailable');
  if(bridge.idle)return {phase:'Idle',can_end:false,launch_locked:false,history_revision:1};
  return {phase:bridge.cleanup?'CleanupPending':'Running',game:'Fixture game',can_end:true,launch_locked:true,elapsed_seconds:1938,captures_saved:3,profile_label:'Session profile',feature_summary:'Metrics enabled · replay enabled',history_revision:1,message:bridge.captureMessage||null,measurements:bridge.noCapture?null:{revision:bridge.revision,...structuredClone(bridge.capture),average_fps:null,frame_time_ms:null,frame_intervals_ns:[]}};
 }
 if(command==='diagnostics_snapshot'){
  bridge.calls.push({command,args});
  const snapshot={hardware_samples:bridge.monitorSamples,game_scans:3,hardware_overruns:0,game_scan_overruns:0,last_hardware_sample_ms:0,last_game_scan_ms:5,captured_at_unix_ms:now(),capture_runtime:'Available',received_frame_count:600,encoded_packet_count:590,audio_packet_count:20,audio_byte_count:1024,shortcut_state:'Active',replay_readiness:{},encoder_candidates:[{driver:'amdgpu',accessible:true}]};
  if(args.refreshCapabilities&&bridge.holdDiagnosticsRefresh)await new Promise(resolve=>bridge.releaseDiagnosticsRefresh=resolve);
  if(!args.refreshCapabilities&&bridge.holdDiagnosticsPoll)await new Promise(resolve=>bridge.releaseDiagnosticsPoll=resolve);
  if(bridge.failDiagnostics)throw Error('Fixture diagnostics unavailable');
  return snapshot;
 }
 return invoke(command,args);
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
 view.load_uri(f'http://127.0.0.1:{server.server_port}/#overview')
 wait_for("typeof q==='function'&&q('#session-name')?.textContent==='Fixture game'")
 test('Armed capture leaves no diagnostic line or empty feedback gap on Overview',"check(q('#session-note').textContent==='','no routine note');check(getComputedStyle(q('.session-feedback')).display==='none','feedback space collapses');check(q('.session-footer').textContent.includes('Session settings')&&q('.session-footer').textContent.includes('3 clips saved'),'session footer kept');")
 snap('overview-armed')
 route('settings')
 test('Diagnostics disclosure starts collapsed and responds to the entire header',"""
  window.foldout=q('.settings-diagnostics');window.foldoutHeader=q('.settings-diagnostics-summary');
  window.foldoutHeaderBox=foldoutHeader.getBoundingClientRect();
  check(!foldout.open,'starts collapsed');
  foldoutHeader.focus();
  click('.diagnostics-chevron');
 """)
 pump(70)
 js("click('.settings-diagnostics-summary');click('.settings-diagnostics-summary')")
 wait_for("foldout.open&&foldout.dataset.expanded===undefined")
 test('Rapid reversal settles open with a stable header and focus',"""
  const box=foldoutHeader.getBoundingClientRect();
  check(box.width===foldoutHeaderBox.width&&box.height===foldoutHeaderBox.height,'header size stays fixed');
  check(document.activeElement===foldoutHeader,'summary keeps keyboard focus');
  check(!q('.diagnostics-panel').inert,'open content is interactive');
  check(getComputedStyle(q('.diagnostics-chevron')).transform!=='none','chevron shows open state');
 """)
 wait_for("q('#diagnostic-capture-phase')?.textContent==='Armed'")
 test('Diagnostics footer names the monitor timings, shortcut owner, and GPU access',"""
  const text=q('[data-diagnostics-details]').textContent;
  check(text.includes('Last monitor update:')&&text.includes('System sampling: 0 ms')&&text.includes('Game scan: 5 ms'),'timings retain their values');
  check(text.includes('Samples or scans over their time limit: 0'),'slow read count');
  check(text.includes('Keyboard shortcuts: Redunar handles its shortcuts while it runs.'),'shortcut copy');
  check(text.includes('Replay GPU device access: amdgpu (access available)'),'device access only');
 """)
 test('Diagnostics keeps zero capture counters separate from replay frames',"check(q('#diagnostic-capture-received').textContent==='0'&&q('#diagnostic-capture-dropped').textContent==='0','zero is real data');check(q('.diagnostics-panel').textContent.includes('Replay frames received'),'separate replay label');window.received=q('#diagnostic-capture-received');window.phase=q('#diagnostic-capture-phase');bridge.capture={phase:'Capturing',received_frame_count:384,dropped_frame_count:3};")
 wait_for("q('#diagnostic-capture-received').textContent==='384'")
 test('Capture counters update in place from current session polling',"check(q('#diagnostic-capture-received')===received&&q('#diagnostic-capture-phase')===phase,'same fields');check(phase.textContent==='Capturing'&&q('#diagnostic-capture-dropped').textContent==='3','phase and drops update');")
 window.resize(1000,900);pump(200);js("q('.diagnostics-panel').scrollIntoView({block:'center'})");snap('diagnostics-capturing')
 test('Diagnostics rows fit within the panel',"const p=q('.diagnostics-panel').getBoundingClientRect();for(const id of ['phase','received','dropped']){const r=q('#diagnostic-capture-'+id).getBoundingClientRect();check(r.left>=p.left&&r.right<=p.right,'counter stays within panel');}check(document.documentElement.scrollWidth<=innerWidth+1,'no page overflow');")
 route('overview')
 test('Capturing does not add routine counters to Overview',"check(q('#session-note').textContent==='','no capture counter note');bridge.capture.phase='Failed';bridge.captureMessage='Capture failed after 384 frames received: EPIPE';")
 wait_for("q('#session-note').textContent===bridge.captureMessage")
 test('Actual capture failures remain visible and accessible',"check(getComputedStyle(q('.session-feedback')).display!=='none','failure visible');check(q('#session-note').getAttribute('role')==='status','status announcement');check(q('#session-note').textContent.includes('EPIPE'),'technical detail retained');bridge.cleanup=true;bridge.captureMessage='Session ended with pending work: fixture EIO. Use End session to retry.';")
 wait_for("q('#session-note').textContent===bridge.captureMessage")
 test('Cleanup warnings retain their recovery action',"check(q('#session-note').textContent.includes('Use End session to retry'),'recovery guidance');check(!q('[data-action=end-session]').hidden&&!q('[data-action=end-session]').disabled,'End session available');")
 snap('overview-cleanup-warning')
 route('settings');js("click('.settings-diagnostics-summary')");pump(250);wait_for("q('#diagnostic-capture-phase')?.textContent==='Failed'")
 test('Failed capture keeps the exact diagnostic counters',"check(q('#diagnostic-capture-received').textContent==='384'&&q('#diagnostic-capture-dropped').textContent==='3','failure counters retained');bridge.noCapture=true;")
 wait_for("q('#diagnostic-capture-received').textContent==='—'")
 test('A missing capture snapshot clears old values without inventing zero',"check(q('#diagnostic-capture-phase').textContent==='—'&&q('#diagnostic-capture-dropped').textContent==='—','missing data shown honestly');bridge.noCapture=false;bridge.captureMessage=null;bridge.cleanup=false;bridge.capture={phase:'Capturing',received_frame_count:401,dropped_frame_count:4};")
 wait_for("q('#diagnostic-capture-received').textContent==='401'")
 test('Refresh keeps the same controls and ignores duplicate requests',"""
  window.refreshControl=q('[data-action=reload-diagnostics]');window.originalFoldout=q('.settings-diagnostics');
  window.refreshCalls=()=>bridge.calls.filter(c=>c.command==='diagnostics_snapshot'&&c.args.refreshCapabilities).length;
  window.priorRefreshes=refreshCalls();
  bridge.holdDiagnosticsRefresh=true;refreshControl.focus();refreshControl.click();refreshControl.click();
  check(refreshCalls()===priorRefreshes+1,'only one explicit request');
  check(refreshControl.getAttribute('aria-disabled')==='true','pending request is announced');
  check(document.activeElement===refreshControl,'pending refresh keeps focus');
 """)
 wait_for("typeof bridge.releaseDiagnosticsRefresh==='function'")
 js("bridge.failDiagnostics=true;bridge.holdDiagnosticsRefresh=false;bridge.releaseDiagnosticsRefresh()")
 wait_for("q('.diagnostics-feedback').textContent.includes('Diagnostics refresh failed')&&!refreshControl.hasAttribute('aria-disabled')")
 test('Failed refresh leaves the last results, live capture counters, and retry available',"""
  check(q('.settings-diagnostics')===originalFoldout&&originalFoldout.open,'same open disclosure');
  check(q('[data-action=reload-diagnostics]')===refreshControl&&document.activeElement===refreshControl,'same focused retry control');
  check(q('[data-diagnostics-snapshot-row]').textContent.includes('12'),'previous snapshot remains');
  check(q('#diagnostic-capture-received').textContent==='401'&&q('#diagnostic-capture-dropped').textContent==='4','independent session data');
 """)
 snap('diagnostics-refresh-failure')
 js("bridge.failDiagnostics=false;bridge.holdDiagnosticsPoll=true;bridge.clockOffset=12000")
 wait_for("typeof bridge.releaseDiagnosticsPoll==='function'")
 js("bridge.monitorSamples=25;refreshControl.click()")
 wait_for("q('[data-diagnostics-snapshot-row] b').textContent==='25'&&!refreshControl.hasAttribute('aria-disabled')")
 js("bridge.holdDiagnosticsPoll=false;bridge.releaseDiagnosticsPoll()");pump(400)
 test('Retry clears the error and an older background result cannot overwrite it',"""
  check(!q('.diagnostics-feedback').textContent,'failure cleared');
  check(q('[data-diagnostics-snapshot-row] b').textContent==='25','new result preserved');
  bridge.failSession=true;
 """)
 wait_for("q('#diagnostic-capture-received').textContent==='—'")
 test('Rejected session reads do not retain stale counters',"check(q('#diagnostic-capture-phase').textContent==='—'&&q('#diagnostic-capture-dropped').textContent==='—','stale values cleared');bridge.failSession=false;bridge.failDiagnostics=false;bridge.idle=true;")
 route('overview');wait_for("q('#session-name')?.textContent==='No game running'")
 test('Idle Overview stays clean after recovery',"check(q('#session-note').textContent===''&&getComputedStyle(q('.session-feedback')).display==='none','no stale warning');check(!bridge.calls.some(c=>['launch_game','end_session'].includes(c.command)),'fixture never launches or ends a game');check(bridge.errors.length===0,bridge.errors.join('; '));")
 view.load_uri(f'http://127.0.0.1:{server.server_port}/?diagnostics-unavailable#settings')
 wait_for("location.search.includes('diagnostics-unavailable')&&q('.diagnostics-feedback')?.textContent.includes('Diagnostics could not load')")
 js("click('.settings-diagnostics-summary')")
 wait_for("q('.settings-diagnostics').open&&q('.settings-diagnostics').dataset.expanded===undefined")
 test('An initial load failure also offers Refresh and can recover',"""
  check(!q('[data-diagnostics-snapshot-row]'),'unavailable values stay absent');
  check(q('[data-action=reload-diagnostics]'),'refresh remains visible');
  bridge.failDiagnostics=false;click('[data-action=reload-diagnostics]');
 """)
 wait_for("q('[data-diagnostics-snapshot-row] b')?.textContent==='12'&&!q('.diagnostics-feedback').textContent")
 (ARTIFACTS/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
 print(f'PASS: {len(checks)} capture diagnostics UI checks.',flush=True)
finally:
 window.destroy();server.shutdown()
