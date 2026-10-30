"""Overview retains its scroll extent through missing memory measurements."""
from webview_fixture import *

extra=r"""
bridge.idle=true;bridge.automaticUpdates=false;
const invoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 const result=await invoke(command,args);
 if(command==='daemon_status'&&bridge.partialMemory){
  result.ram_used_bytes=null;result.vram_used_bytes=null;
 }
 return result;
};
window.scrollGeometry=()=>{
 const viewport=q('.window-content');
 return [viewport.scrollTop,viewport.scrollHeight,q('.recent-captures').getBoundingClientRect().top];
};
window.checkGeometry=()=>{
 const current=scrollGeometry();
 check(current.every((value,i)=>Math.abs(value-beforeGeometry[i])<1),
  'scroll geometry changed: '+JSON.stringify({before:beforeGeometry,after:current}));
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#overview');pump(1200)
    wait_for("qa('.recent-thumbnail img').length===3")
    for width in (1440,1100,900):
        window.resize(width,800);pump(300)
        js("q('.window-content').scrollTop=10000;window.beforeGeometry=scrollGeometry();bridge.failDaemon=true;")
        wait_for("q('#ram-total').textContent==='Capacity unavailable'")
        test(f'{width}px monitor loss retains page height and bottom position',"checkGeometry();check(q('#ram-used').textContent==='—','missing reading remains unavailable');check(q('#ram-available').textContent==='','no stale availability');bridge.failDaemon=false;")
        wait_for("q('#ram-total').textContent!=='Capacity unavailable'")
        test(f'{width}px monitor recovery retains content position',"checkGeometry();check(q('#ram-available').textContent.includes('available'),'readings recover');bridge.partialMemory=true;")
        wait_for("q('#ram-used').textContent==='—'")
        test(f'{width}px partial memory loss retains content position',"checkGeometry();check(q('#ram-available').textContent==='','missing RAM availability cleared');check(q('#vram-available').textContent==='','missing VRAM availability cleared');bridge.partialMemory=false;")
        wait_for("q('#ram-used').textContent!=='—'")
        test(f'{width}px partial memory recovery retains content position',"checkGeometry();")
    js("q('.system-panel').scrollIntoView({block:'center'})");pump(100)
    snap('overview-scroll-stable')
    test('Overview has no fixture errors',"check(bridge.errors.length===0,bridge.errors.join('; '));")
    (ARTIFACTS/'overview-scroll-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} Overview scroll checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
