"""Interaction motion and cancellation in production WebKit with private state."""
from webview_fixture import *

# Change only this fixture process's GTK preference, never desktop settings.
gtk_settings = Gtk.Settings.get_default()
original_animations = gtk_settings.get_property('gtk-enable-animations')
gtk_settings.set_property('gtk-enable-animations', True)
extra = r"""
bridge.idle=true;bridge.automaticUpdates=false;
bridge.motionStarts=[];
document.addEventListener('animationstart',event=>bridge.motionStarts.push(event.animationName));
window.motion=(element,name)=>element.getAnimations().find(a=>a.animationName===name);
window.editScale=value=>{const input=q('[data-global=scale]');input.value=value;input.dispatchEvent(new Event('input',{bubbles:true}));};
window.key=(target,key)=>target.dispatchEvent(new KeyboardEvent('keydown',{key,bubbles:true,cancelable:true}));
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#global')
    pump(500)
    wait_for("q('[data-global=scale]')?.value==='100'&&matchMedia('(prefers-reduced-motion: no-preference)').matches")
    pump()
    test('Clean settings have no draft entry animation', "check(q('.settings-save-bar').hidden,'clean bar hidden');check(!motion(q('.settings-save-bar'),'save-bar-enter'),'no hidden animation');")
    test('First edit animates usable draft actions', "editScale('105');window.bar=q('.settings-save-bar');const a=motion(bar,'save-bar-enter');check(a,'entry exists');check(a.effect.getTiming().duration<=140,'bounded duration');check(!bar.hidden&&!q('[data-action=save-global]').disabled,'actions immediately usable');a.pause();a.currentTime=70;")
    snap('motion-save-bar-entry')
    js("motion(bar,'save-bar-enter').play()")
    pump(250)
    test('Finished entry releases transforms and layer hints', "check(!motion(bar,'save-bar-enter'),'finished animation removed');check(getComputedStyle(bar).transform==='none','no retained transform');check(getComputedStyle(bar).willChange==='auto','no persistent layer hint');window.entryCount=bridge.motionStarts.filter(n=>n==='save-bar-enter').length;window.pollCount=bridge.calls.filter(c=>c.command==='daemon_status').length;editScale('110');")
    pump(2300)
    test('Further edits and real polling do not restart entry', "check(bridge.calls.filter(c=>c.command==='daemon_status').length>pollCount,'polling ran');check(q('.settings-save-bar')===bar,'bar retained');check(bridge.motionStarts.filter(n=>n==='save-bar-enter').length===entryCount,'no repeat animation');check(!motion(bar,'save-bar-enter'),'no idle animation');")
    js("click('[data-global-tab=replay]')")
    pump()
    test('Tab renders retain the draft without replaying entry', "check(!q('.settings-save-bar').hidden,'draft retained');check(bridge.motionStarts.filter(n=>n==='save-bar-enter').length===entryCount,'render did not restart');bridge.conflict=true;click('[data-action=save-global]');")
    wait_for("q('#toast').textContent.includes('changed elsewhere')")
    test('Failed save retains immediately usable retry without entry', "check(!q('.settings-save-bar').hidden,'draft retained');check(!q('[data-action=save-global]').disabled,'retry enabled');check(!motion(q('.settings-save-bar'),'save-bar-enter'),'failure did not restart');bridge.conflict=false;click('[data-action=discard-global]');")
    test('Discard hides actions without a closing delay', "check(q('.settings-save-bar').hidden,'immediate hide');check(document.body.dataset.unsaved==='false','clearance restored');check(!q('.settings-save-bar').getAnimations().length,'no hidden animation');click('[data-global-tab=overlay]');editScale('105');check(motion(q('.settings-save-bar'),'save-bar-enter'),'new draft animates');click('[data-action=discard-global]');check(q('.settings-save-bar').hidden&&!q('.settings-save-bar').getAnimations().length,'interrupted entry removed');")
    route('library')
    test('Inherited game drafts get their own entry and immediate discard', "const s=q('[data-override=overlay]');s.value='Off';s.dispatchEvent(new Event('change',{bubbles:true}));const b=q('.game-save-bar');check(motion(b,'save-bar-enter'),'game entry');check(!q('[data-action=save-game]').disabled,'save ready');click('[data-action=discard-game]');check(q('[data-override=overlay]').value==='inherit','inheritance restored');check(q('.game-save-bar').hidden,'immediate game hide');")
    test('Dialogs fade without moving their positioning context', "q('[data-action=add-game]').focus();click('[data-action=add-game]');const d=q('#dialog'),a=motion(d,'dialog-enter');check(a,'dialog entry');check(a.effect.getTiming().duration<=140,'bounded dialog');check(document.activeElement===q('#dialog-title'),'neutral focus');check(getComputedStyle(d).transform==='none','fixed-menu context stable');q('#dialog [name=name]').value='Retained fixture draft';a.pause();a.currentTime=70;")
    snap('motion-dialog-entry')
    test('Dialog entry can be cancelled immediately with focus restored', "click('#dialog [data-close]');check(!q('#dialog').open,'closed now');check(!q('#dialog').getAnimations().length,'animation cancelled');check(document.activeElement===q('[data-action=add-game]'),'invoker focus restored');click('[data-action=add-game]');check(motion(q('#dialog'),'dialog-enter'),'reopen animates');")
    pump(180)
    test('Completed dialog is fully visible without a retained animation', "check(getComputedStyle(q('#dialog')).opacity==='1','readable final state');check(!q('#dialog').getAnimations().length,'dialog animation released');")
    snap('motion-dialog-ready')
    js("click('#dialog [data-close]')")
    route('global')
    test('Dropdown entry preserves placement and keyboard ownership', "window.select=q('[data-global=position]');window.trigger=select.nextElementSibling;trigger.focus();key(trigger,'ArrowDown');const menu=q('.precision-select-menu'),a=motion(menu,'dropdown-enter');check(a,'menu entry');check(a.effect.getTiming().duration<=100,'bounded menu');check(document.activeElement===trigger,'keyboard focus retained');const r=trigger.getBoundingClientRect(),m=menu.getBoundingClientRect();check(m.left>=6&&m.right<=innerWidth-6,'menu fits horizontally');check(m.top>=r.bottom,'menu below trigger');a.pause();a.currentTime=50;")
    snap('motion-dropdown-entry')
    test('Keyboard selection commits during entry and removes the menu', "key(trigger,'End');key(trigger,'Enter');check(select.value==='Bottom right','selection committed');check(!q('.precision-select-menu'),'no delayed removal');check(document.activeElement===trigger,'focus retained');trigger.click();check(motion(q('.precision-select-menu'),'dropdown-enter'),'new opening animates');key(trigger,'Escape');check(!q('.precision-select-menu'),'Escape immediate');check(trigger.getAttribute('aria-expanded')==='false','accessible state closed');")
    test('Scroll dismisses a dropdown even during entry', "trigger.click();check(q('.precision-select-menu'),'open');q('.window-content').dispatchEvent(new Event('scroll'));check(!q('.precision-select-menu'),'scroll closes menu');click('[data-action=discard-global]');")
    window.resize(640,720)
    pump()
    test('Narrow entry stays within the viewport', "editScale('105');const b=q('.settings-save-bar'),a=motion(b,'save-bar-enter');a.pause();a.currentTime=70;const r=b.getBoundingClientRect();check(r.left>=0&&r.right<=innerWidth&&r.bottom<=innerHeight,'bar fits');check(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow');")
    snap('motion-save-bar-narrow')
    js("click('[data-action=discard-global]')")
    gtk_settings.set_property('gtk-enable-animations', False)
    # Evaluate the production stylesheet afresh under the reduced preference.
    view.reload();pump(500)
    wait_for("q('[data-global=scale]')?.value==='100'&&matchMedia('(prefers-reduced-motion: reduce)').matches")
    test('Reduced motion keeps draft actions immediate and stationary', "editScale('105');const b=q('.settings-save-bar');check(!b.hidden&&!q('[data-action=save-global]').disabled,'actions ready');check(!b.getAnimations().length&&getComputedStyle(b).transform==='none','no bar motion: '+JSON.stringify({name:getComputedStyle(b).animationName,transform:getComputedStyle(b).transform,animations:b.getAnimations().map(a=>({name:a.animationName,state:a.playState}))}));click('[data-action=discard-global]');window.trigger=q('[data-global=position]').nextElementSibling;trigger.focus();trigger.click();check(!q('.precision-select-menu').getAnimations().length,'no dropdown motion');key(trigger,'Escape');")
    route('library')
    test('Reduced motion dialogs retain opening focus and cancellation', "q('[data-action=add-game]').focus();click('[data-action=add-game]');check(q('#dialog').open&&!q('#dialog').getAnimations().length,'no dialog motion');check(document.activeElement===q('#dialog-title'),'neutral focus');click('#dialog [data-close]');check(document.activeElement===q('[data-action=add-game]'),'focus restored');check(bridge.errors.length===0,bridge.errors.join('; '));")
    pump(300)
    test('Settled workspace has no interaction animation work', "check(!document.getAnimations().some(a=>['dialog-enter','dropdown-enter','save-bar-enter'].includes(a.animationName)),'no residual interaction animation');")
    (ARTIFACTS/'motion-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} motion checks. Screenshots: {ARTIFACTS}')
finally:
    gtk_settings.set_property('gtk-enable-animations', original_animations)
    window.destroy();server.shutdown()
