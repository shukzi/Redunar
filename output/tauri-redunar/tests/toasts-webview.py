"""Production notification pills with isolated commands and controlled wall time."""
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
const fixtureRealNow=Date.now;
window.fixtureWallOffset=0;Date.now=()=>fixtureRealNow()+fixtureWallOffset;
window.fixtureToast=(message,options)=>q('#toast')[Symbol.for('redunar.toast-owner')](message,options);
window.liveToasts=()=>qa('.toast-card');
window.clearToasts=()=>liveToasts().forEach(card=>card.querySelector('button').click());
window.foregroundToast=()=>{q('#workspace').focus({preventScroll:true});window.dispatchEvent(new Event('focus'));};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))

try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#settings')
    wait_for("typeof q==='function'&&q('[data-action=check-for-updates]')&&!q('[data-action=check-for-updates]').disabled")
    js("foregroundToast();click('[data-action=check-for-updates]')")
    wait_for("liveToasts()[0]?.textContent.includes('Verified update 0.1.1')")
    pump(180)
    test('Real update responses use one outlined bottom-center pill', """
     const card=liveToasts()[0],box=card.getBoundingClientRect(),css=getComputedStyle(card);
     check(liveToasts().length===1,'one current pill');
     check(Math.abs((box.left+box.right)/2-innerWidth/2)<1,'centered in window');
     check(css.borderTopColor==='rgb(82, 82, 82)'&&css.backgroundColor==='rgb(22, 22, 22)','outlined graphite');
     check(!q('.toast-progress')&&!q('.toast-icon'),'no countdown or leading icon');
     check(box.width<=560&&box.height>=38,'compact bounded surface');
     check(bridge.calls.some(call=>call.command==='check_for_updates'),'production action used');
     clearToasts();fixtureToast('Settings saved',{type:'success'});
    """)
    pump(180);snap('outlined-pill-settings')
    test('A duplicate refreshes the same mounted pill without another animation', """
     window.previousPill=liveToasts()[0];fixtureToast('Settings saved',{type:'success'});
     check(liveToasts().length===1&&liveToasts()[0]===previousPill,'mounted message reused');
     check(previousPill.getAnimations().length===0,'no repeated entrance');
     window.staleDismiss=previousPill.querySelector('button');
     fixtureToast('Latest message');staleDismiss.click();
     check(liveToasts().length===1&&liveToasts()[0].textContent.includes('Latest message'),'stale dismiss cannot remove replacement');
    """)
    test('Bursts leave no backing surfaces, outgoing animations, or queue', """
     for(let i=0;i<100;i++)fixtureToast('Message '+i);
     check(liveToasts().length===1&&liveToasts()[0].textContent.includes('Message 99'),'latest only');
     check(q('#toast').children.length===1,'bounded DOM');
     fixtureWallOffset+=10000;window.dispatchEvent(new Event('focus'));
     check(q('#toast').hidden&&!liveToasts().length,'no backlog after expiry');
    """)
    test('Notification content remains literal text', """
     const text='<img src=x onerror="throw Error(1)">';fixtureToast(text,{type:'error'});
     check(q('.toast-message').textContent===text&&!q('#toast img'),'text cannot become markup');
     check(liveToasts()[0].getAttribute('role')==='alert','error announced');clearToasts();
    """)
    test('Keyboard dismissal restores the invoking control', """
     const origin=q('[data-action=check-for-updates]');origin.focus();
     fixtureToast('Keyboard message');q('.toast-dismiss').focus();
     q('.toast-dismiss').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
     check(q('#toast').hidden&&!liveToasts().length,'Escape removes pill immediately');
     check(document.activeElement===origin,'invoking focus restored');
    """)
    test('A foreground reading hold releases immediately when the window loses focus', """
     foregroundToast();fixtureToast('Read this',{type:'success'});
     q('#toast').dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));
     fixtureWallOffset+=10000;
    """)
    pump(1100)
    test('Reading never extends a background deadline', """
     check(liveToasts().length===1,'foreground reading retained');
     window.dispatchEvent(new Event('blur'));
     check(q('#toast').hidden&&!liveToasts().length,'expired hold cleared on blur');
     window.dispatchEvent(new Event('focus'));
     check(q('#toast').hidden,'no old message on return');
    """)
    test('A focused pill also expires on blur without moving focus back into the app', """
     foregroundToast();fixtureToast('Focused reading');q('.toast-dismiss').focus();
     fixtureWallOffset+=10000;window.dispatchEvent(new Event('blur'));
     check(q('#toast').hidden&&!liveToasts().length,'focused hold cleared');
     check(document.activeElement!==q('[data-action=check-for-updates]'),'background expiry does not steal focus');
    """)
    window.hide();pump(100)
    js("fixtureToast('Created in background',{duration:1000})")
    pump(1300)
    window.show_all();pump(180)
    test('A message created in the hidden WebKit view expires before returning', "check(q('#toast').hidden&&!liveToasts().length,'no hidden backlog');")
    test('Page-cache restoration rechecks delayed expiry before displaying anything', """
     foregroundToast();fixtureToast('Before page cache');
     window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true}));
     fixtureWallOffset+=10000;
     window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true}));
     check(q('#toast').hidden&&!liveToasts().length,'expired cached page cleared');
     fixtureToast('Fresh after return');check(liveToasts().length===1,'owner retained for cached page');clearToasts();
    """)
    route('library');js("fixtureToast('Existing before dialog',{duration:6000});click('[data-action=add-game]')")
    wait_for("q('#toast').parentElement===q('#dialog')")
    test('Opening a dialog moves an existing pill into its top layer', "check(q('#dialog').open&&!q('#toast').hidden,'existing notification stays visible');click('#dialog [data-close]');")
    wait_for("q('#toast').parentElement===document.body")
    test('Rapid dialog close and reopen keeps current ownership', """
     click('[data-action=add-game]');click('#dialog [data-close]');click('[data-action=add-game]');
     fixtureToast('Reopened dialog');
    """)
    pump(180)
    test('A stale close cannot put a new dialog pill behind it', "check(q('#dialog').open&&q('#toast').parentElement===q('#dialog'),'current dialog retains pill');click('#dialog [data-close]');")
    wait_for("q('#toast').parentElement===document.body")
    js('clearToasts()');route('global')
    wait_for("q('[data-global=overlay]')&&!q('[data-global=overlay]').disabled")
    js("q('[data-global=overlay]').click();fixtureToast('Draft remains pending',{duration:6000})")
    pump(220)
    test('The pill clears actual Save and Discard actions without changing the draft', """
     const bar=q('[data-action=save-global]').closest('.change-save-bar'),pill=liveToasts()[0].getBoundingClientRect();
     check(document.body.dataset.unsaved==='true'&&bar,'real draft visible');
     check(pill.bottom<bar.getBoundingClientRect().top,'notification above draft actions');
     check(q('[data-global=overlay]').checked!==values.overlay,'draft retained');
    """)
    snap('outlined-pill-draft')
    js("click('[data-action=discard-global]')");pump(180)
    test('Discard retains the saved setting and clears only the draft bar', "check(q('[data-global=overlay]').checked===values.overlay,'saved setting restored');check(document.body.dataset.unsaved!=='true','draft cleared');clearToasts();")
    window.resize(480,800);pump(250)
    js("fixtureToast('A_long_unbroken_message_'.repeat(12),{type:'error',duration:6000})");pump(180)
    test('Long messages and dismissal fit a narrow window', """
     const box=liveToasts()[0].getBoundingClientRect(),close=q('.toast-dismiss').getBoundingClientRect();
     check(box.left>=15&&box.right<=innerWidth-15,'pill fits narrow window');
     check(close.left>=box.left&&close.right<=box.right,'dismiss fits pill');
     check(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow');clearToasts();
    """)
    window.resize(1440,1000);pump(180)
    Gtk.Settings.get_default().set_property('gtk-enable-animations',False)
    wait_for("matchMedia('(prefers-reduced-motion: reduce)').matches")
    pump(250)
    test('Reduced motion has no entrance or retained exit animation', """
     fixtureToast('Reduced motion');check(getComputedStyle(liveToasts()[0]).animationName==='none','no CSS entrance: '+JSON.stringify({animation:getComputedStyle(liveToasts()[0]).animationName,reduce:matchMedia('(prefers-reduced-motion: reduce)').matches,normal:matchMedia('(prefers-reduced-motion: no-preference)').matches}));
     check(liveToasts()[0].getAnimations().length===0,'no animation');
     clearToasts();check(q('#toast').hidden&&!liveToasts().length,'no retained outgoing surface');
    """)
    test('Page disposal disconnects ownership and rejects late notifications', """
     window.lateToastOwner=q('#toast')[Symbol.for('redunar.toast-owner')];
     fixtureToast('Before disposal');
     window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:false}));
     lateToastOwner('Late async result');
     check(!q('#toast')[Symbol.for('redunar.toast-owner')]&&q('#toast').hidden&&!liveToasts().length,'disposed owner stays stopped');
     check(bridge.errors.length===0,bridge.errors.join('; '));
    """)
    (ARTIFACTS/'toasts-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} pill checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
