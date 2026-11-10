"""Exercise permanent deletion against synthetic native responses only."""
from webview_fixture import *
ARTIFACTS = NATIVE.parents[1] / '.redunar-build/reports/trash-bin-ui'
ARTIFACTS.mkdir(parents=True,exist_ok=True)
extra=r"""
bridge.startupUpdateAvailable=false;
bridge.trash=[{file_name:'A long recording filename from a local fixture.mkv',bytes:12000000},{file_name:'Second clip.mkv',bytes:8000000}];
const originalInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='trashed_replay_clips')return structuredClone(bridge.trash);
 if(command==='prepare_trash_deletion'){
  bridge.preview={token:String((Number(bridge.preview?.token)||0)+1),entries:structuredClone(bridge.trash.filter(c=>args.fileName===null||c.file_name===args.fileName))};
  return bridge.preview;
 }
 if(command==='delete_trashed_replay_clips'){
  bridge.deleteCalls=(bridge.deleteCalls||0)+1;
  await new Promise(resolve=>setTimeout(resolve,150));
  if(bridge.deleteError)throw Error('Fixture deletion failed. Refresh the bin.');
  const names=bridge.preview.entries.map(c=>c.file_name),deleted=bridge.partial?names.slice(0,1):names;
  bridge.trash=bridge.trash.filter(c=>!deleted.includes(c.file_name));
  return {deleted,failures:bridge.partial?names.slice(1).map(n=>[n,'The clip was restored. It was not deleted.']):[]};
 }
 return originalInvoke(command,args);
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
def capture(name):
    loop=GLib.MainLoop()
    def done(w,value,_):
        w.get_snapshot_finish(value).write_to_png(str(ARTIFACTS/(name+'.png')));loop.quit()
    view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE,WebKit2.SnapshotOptions.NONE,None,done,None);loop.run()
try:
    window.resize(1280,900);view.load_uri(f'http://127.0.0.1:{server.server_port}/#replay');pump(1200)
    js("click('[data-action=view-clip-trash]')");pump(250)
    test('Restore and permanent delete coexist in each row',"check(qa('.trash-bin-row').length===2,'two rows');check(qa('[data-action=restore-trash-clip]').length===2,'restore retained');check(qa('[data-action=delete-trash-clip]').length===2,'delete available');")
    capture('bin-wide')
    window.resize(640,720);pump(250)
    test('Narrow bin has no horizontal overflow',"const d=q('#dialog');check(d.scrollWidth<=d.clientWidth,'dialog width');")
    capture('bin-narrow')
    js("click('[data-action=delete-trash-clip]')");pump(250)
    test('Single deletion requires exact confirmation with neutral focus',"check(bridge.preview.entries.length===1,'one target');check(q('#dialog-title')===document.activeElement,'neutral focus');check(!bridge.deleteCalls,'no premature deletion');")
    capture('single-confirmation')
    js("click('[data-action=view-clip-trash]')");pump(250)
    test('Cancel does not delete',"check(bridge.trash.length===2&&!bridge.deleteCalls,'unchanged bin');")
    js("click('[data-action=clear-trash-bin]')");pump(250)
    test('Clear bin confirms all listed files',"check(bridge.preview.entries.length===2,'two targets');check(q('.trash-delete-list').textContent.includes('Second clip.mkv'),'names visible');")
    capture('clear-confirmation')
    js("bridge.partial=true;click('[data-action=confirm-trash-deletion]');click('[data-action=confirm-trash-deletion]')");pump(450)
    test('Double click runs one deletion and preserves partial failures',"check(bridge.deleteCalls===1,'one request');check(bridge.trash.length===1,'failed clip retained');check(q('#dialog-body').textContent.includes('was restored'),'failure visible');")
    capture('partial-failure')
    js("bridge.partial=false;click('[data-action=view-clip-trash]')");pump(200)
    js("click('[data-action=clear-trash-bin]')");pump(200)
    js("bridge.deleteError=true;click('[data-action=confirm-trash-deletion]')");pump(350)
    test('Native failure remains visible without false success',"check(!q('#dialog-error').hidden,'error visible');check(bridge.trash.length===1,'recording remains');")
    js("bridge.deleteError=false;click('[data-action=view-clip-trash]')");pump(200)
    js("click('[data-action=clear-trash-bin]')");pump(200)
    js("click('[data-action=confirm-trash-deletion]')");pump(350)
    js("click('[data-action=view-clip-trash]')");pump(200)
    test('Empty bin omits destructive actions',"check(!q('[data-action=clear-trash-bin]')&&!q('[data-action=delete-trash-clip]'),'empty controls');check(bridge.errors.length===0,bridge.errors.join('; '));")
    capture('empty-bin')
    (ARTIFACTS/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} trash-bin checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
