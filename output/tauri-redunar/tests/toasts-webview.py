"""Toast stack expiry and dismissal in production WebKit with private updates."""
import webview_fixture as harness
from webview_fixture import *

ARTIFACTS=NATIVE.parents[1]/'.redunar-build/reports/toasts-ui'
ARTIFACTS.mkdir(parents=True,exist_ok=True)
harness.ARTIFACTS=ARTIFACTS
Gtk.Settings.get_default().set_property('gtk-enable-animations',True)
extra=r"""
bridge.idle=true;bridge.automaticUpdates=false;bridge.startupUpdateAvailable=false;
bridge.updateMessage='Cannot check for newer updates. Verified update 0.1.1 is still available.';
const originalInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>command==='check_for_updates'?
  (bridge.calls.push({command,args}),{current_version:'0.1.0',latest_version:'0.1.1',state:'available',message:bridge.updateMessage}):originalInvoke(command,args);
window.fixtureToast=(message,options)=>q('#toast')[Symbol.for('redunar.toast-owner')](message,options);
window.liveToasts=()=>qa('.toast-card:not([aria-hidden="true"])');
window.clearToasts=()=>liveToasts().forEach(card=>card.querySelector('button').click());
window.exitFrame=(card,time=100)=>{
 const animation=card.getAnimations()[0];check(animation,'exit animation');animation.pause();animation.currentTime=time;return animation;
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))

try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#settings')
    wait_for("typeof q==='function'&&q('[data-action=check-for-updates]')&&!q('[data-action=check-for-updates]').disabled")
    js("window.dispatchEvent(new Event('focus'));fixtureToast('Update available. Open Settings to install it.',{duration:1100});")
    pump(700)
    js("click('[data-action=check-for-updates]')")
    wait_for("liveToasts().some(card=>card.textContent.includes('Verified update 0.1.1'))")
    wait_for("q('.toast-card[aria-hidden=true]')")
    js("window.expiring=q('.toast-card[aria-hidden=true]');window.expiryMotion=exitFrame(expiring,100)")
    snap('update-expiry-midpoint')
    test('An older update notification expires without crossing below the stack', "check(expiring.getBoundingClientRect().bottom<=q('#toast').getBoundingClientRect().bottom+1,'departing card spills below the active update');check(liveToasts().length===1&&liveToasts()[0].textContent.includes('Verified update 0.1.1'),'verified update stays readable');check(bridge.calls.some(call=>call.command==='check_for_updates'),'real update action used');expiryMotion.finish();")
    pump(80)
    test('Expired backing surfaces and their animation jobs are removed', "check(!expiring.isConnected&&!q('.toast-card[aria-hidden=true]'),'no ghost backing card');check(liveToasts().length===1,'current toast retained');click('[data-action=check-for-updates]');")
    pump(100)
    test('Repeating the same update response refreshes one notification', "check(liveToasts().length===1&&!q('.toast-card[aria-hidden=true]'),'deduplicated response');fixtureToast('Another settings change',{duration:10000});")
    pump(1000)
    test('A shorter notification retains the approved collapsed stack above it', "const front=liveToasts().find(card=>card.dataset.behind==='false'),back=liveToasts().find(card=>card.dataset.behind==='true');check(front&&back,'two stacked cards');check(back.getBoundingClientRect().bottom<front.getBoundingClientRect().bottom,'backing card remains above');q('#toast').dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));")
    pump(900)
    test('Expanded cards stay readable and independently dismissible', "check(q('#toast').dataset.expanded==='true','stack unfolds');check(liveToasts().every(card=>!card.inert&&card.dataset.behind==='false'),'expanded controls usable');window.older=liveToasts()[0];window.olderBottom=older.getBoundingClientRect().bottom;older.querySelector('button').click();window.olderExit=exitFrame(older,150);check(older.getBoundingClientRect().bottom<=olderBottom+1,'older expanded card fades in place');olderExit.finish();")
    pump(80)
    js("q('#toast').dispatchEvent(new PointerEvent('pointerleave',{pointerType:'mouse'}));fixtureToast('Latest notification',{duration:10000})")
    pump(1000)
    test('Dismissing the front of a remaining stack leaves no strip beneath it', "window.front=liveToasts().at(-1);front.querySelector('button').click();window.frontExit=exitFrame(front,100);check(front.getBoundingClientRect().bottom<=q('#toast').getBoundingClientRect().bottom+1,'front exit crosses stack baseline');frontExit.finish();")
    pump(80)
    test('The last toast retains its normal downward dismissal', "window.last=liveToasts()[0];last.querySelector('button').click();window.lastExit=exitFrame(last,100);check(last.getBoundingClientRect().bottom>q('#toast').getBoundingClientRect().bottom,'last card leaves toward edge');check(lastExit.effect.getTiming().duration===200,'bounded exit');lastExit.finish();")
    pump(80)
    test('The viewport clears completely after the last exit', "check(q('#toast').hidden&&!q('.toast-card'),'no empty viewport or lingering surface');")
    route('library');js("click('[data-action=add-game]');fixtureToast('Dialog notification',{duration:10000})");pump(1000)
    test('Dialog notifications stay in the top layer', "check(q('#toast').parentElement===q('#dialog'),'dialog owns notification');click('#dialog [data-close]');")
    wait_for("q('#toast').parentElement===document.body")
    test('Closing a dialog keeps its notification visible in the document', "check(!q('#dialog').open&&!q('#toast').hidden,'closed dialog releases visible notification');check(liveToasts()[0].textContent.includes('Dialog notification'),'message retained');clearToasts();")
    pump(250)
    js("click('[data-action=add-game]');fixtureToast('Previous dialog');click('#dialog [data-close]');click('[data-action=add-game]');fixtureToast('Reopened dialog');")
    pump(80)
    test('A stale close event cannot move a reopened dialog notification behind it', "check(q('#dialog').open&&q('#toast').parentElement===q('#dialog'),'current dialog retains ownership');click('#dialog [data-close]');")
    wait_for("q('#toast').parentElement===document.body")
    js('clearToasts()');pump(250)
    Gtk.Settings.get_default().set_property('gtk-enable-animations',False)
    wait_for("matchMedia('(prefers-reduced-motion: reduce)').matches")
    pump(250)
    test('Reduced motion removes stacked and last notifications immediately', "fixtureToast('Reduced one');fixtureToast('Reduced two');clearToasts();check(q('#toast').hidden&&!q('.toast-card'),'no retained reduced-motion exit');check(bridge.errors.length===0,bridge.errors.join('; '));")
    (ARTIFACTS/'toasts-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} toast checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
