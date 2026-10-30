"""Interaction motion and cancellation in production WebKit with private state.

Use --source-controls for only the current control modules, without a UI build.
"""
import base64
import sys
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

def source_control_checks():
    """Exercise the current control modules without rebuilding or trusting ui/dist."""
    def module_url(name):
        return 'data:text/javascript;base64,' + base64.b64encode((NATIVE/'ui'/name).read_bytes()).decode()
    styles = '\n'.join((NATIVE/'ui'/name).read_text() for name in
                       ['app.css', 'controls.css', 'precision-selects.css', 'workspace-motion.css', 'workspace-polish.css'])
    html = '''<!doctype html><html data-page="global"><head><style>''' + styles + '''
      body { padding:32px; } .tabbar { margin-bottom:24px; }
      #dirty[hidden] { display:none; } #dirty { margin-left:12px; }
      </style></head><body>
      <div class="tabbar"><button data-global-tab="overlay" class="active">Overlay<span id="dirty" hidden>●</span></button><button data-global-tab="replay">Replay</button></div>
      <div class="clip-collection"><button data-clip-scope="all" aria-pressed="true">All</button><button data-clip-scope="favorites">Favorites</button></div>
      <button id="unrelated">Unrelated action</button>
      <select id="source-select" aria-label="Source control">''' + ''.join(
          f'<option value="{n}">Option {n}</option>' for n in range(10)) + '''</select>
      <select id="short-select" aria-label="Short control"><option>One</option><option>Two</option></select>
      <script>window.presentationFrames=0;const originalRaf=requestAnimationFrame;
      window.requestAnimationFrame=callback=>{presentationFrames++;return originalRaf.call(window,callback);};</script>
      <script type="module">
      import {enhancePrecisionSelects} from "''' + module_url('precision-selects.js') + '''";
      import {refreshWorkspacePresentation} from "''' + module_url('workspace-presentation.mjs') + '''";
      enhancePrecisionSelects();refreshWorkspacePresentation();window.sourceControlsReady=true;
      document.addEventListener('click',event=>{
       const tab=event.target.closest('.tabbar>button');
       if(tab)for(const button of document.querySelectorAll('.tabbar>button'))button.classList.toggle('active',button===tab);
      });
      </script></body></html>'''
    # A distinct document URI makes the later /#global load a full navigation.
    # Using / here lets WebKit treat it as a fragment change and retain this DOM.
    view.load_html(html, f'http://127.0.0.1:{server.server_port}/__source-controls')
    wait_for('window.sourceControlsReady')
    pump(300)
    js("window.initialFrames=presentationFrames;click('#unrelated');click('#source-select+.precision-select-trigger');")
    pump(250)
    test('Source unrelated actions and dropdown opening do not schedule indicator frames', "check(presentationFrames===initialFrames,'unrelated click refreshed indicators');check(q('.precision-select-menu.precision-select-scrolls'),'long menu overflow detected');window.trigger=q('#source-select+.precision-select-trigger');window.pageTop=document.scrollingElement.scrollTop;key(trigger,'End');")
    pump(120)
    test('Source highlight keeps the last option visible and its hover plate aligned', "const menu=q('.precision-select-menu'),item=menu.querySelector('.active'),box=item.getBoundingClientRect(),bounds=menu.getBoundingClientRect();check(menu.scrollTop>0&&box.top>=bounds.top&&box.bottom<=bounds.bottom+1,'keyboard scroll stays in popup');check(Math.abs(new DOMMatrix(getComputedStyle(menu,'::before').transform).m42-item.offsetTop)<2,'plate tracks option');check(document.scrollingElement.scrollTop===pageTop,'page did not scroll');key(trigger,'Enter');window.value=trigger.querySelector('.precision-select-value');window.feedback=value.getAnimations().find(a=>a.id==='control-value-commit');check(feedback&&feedback.effect.getTiming().duration===150,'bounded WAAPI commit');feedback.pause();")
    test('Source commits preserve selection, focus and immediate dismissal', "check(q('#source-select').value==='9'&&!q('.precision-select-menu'),'selected value committed');check(document.activeElement===trigger,'focus restored');trigger.click();key(trigger,'Home');key(trigger,'Enter');check(feedback.playState==='idle','previous feedback cancelled');check(value.getAnimations().filter(a=>a.id==='control-value-commit').length===1,'only latest feedback retained');")
    pump(250)
    test('Source feedback finishes without retained transforms or animations', "check(!value.getAnimations().length&&getComputedStyle(value).transform==='none','feedback released');click('#short-select+.precision-select-trigger');check(!q('.precision-select-menu').classList.contains('precision-select-scrolls'),'short menu has no reserved gutter');key(q('#short-select+.precision-select-trigger'),'Escape');")
    js("window.initialFrames=presentationFrames;click('[data-global-tab=replay]');")
    pump(250)
    test('Source selection clicks still refresh the indicator', "check(presentationFrames>initialFrames,'selection click refreshed');const tab=q('.tabbar button.active'),line=q('.selection-indicator-tabs');check(Math.abs(tab.getBoundingClientRect().left-line.getBoundingClientRect().left)<2,'selection geometry current');q('#dirty').hidden=false;")
    pump(250)
    test('Source child resize refreshes geometry without a click', "const tab=q('.tabbar button.active'),line=q('.selection-indicator-tabs');check(Math.abs(tab.getBoundingClientRect().left-line.getBoundingClientRect().left)<2,'preceding dirty marker moves active underline');trigger.click();key(trigger,'End');key(trigger,'Enter');window.feedback=value.getAnimations().find(a=>a.id==='control-value-commit');feedback.pause();")
    gtk_settings.set_property('gtk-enable-animations', False)
    wait_for("matchMedia('(prefers-reduced-motion: reduce)').matches&&feedback.playState==='idle'")
    test('Source reduced-motion change cancels running commit feedback', "check(!value.getAnimations().length,'running commit cancelled');trigger.click();key(trigger,'Home');key(trigger,'Enter');check(q('#source-select').value==='0'&&!value.getAnimations().length,'reduced motion commits immediately');check(!q('.precision-select-menu')&&document.activeElement===trigger,'dismissal and focus preserved');check(!bridge.errors.length,bridge.errors.join('; '));window.initialFrames=presentationFrames;")
    pump(250)
    test('Source controls settle without recurring frame work', "check(presentationFrames===initialFrames,'no idle frame loop');")

try:
    source_control_checks()
    if '--source-controls' in sys.argv:
        (ARTIFACTS/'source-control-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
        print(f'PASS: {len(checks)} direct-source control checks (no frontend build).')
        raise SystemExit(0)
    gtk_settings.set_property('gtk-enable-animations', True)
    wait_for("matchMedia('(prefers-reduced-motion: no-preference)').matches")
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#global')
    pump(500)
    wait_for("!window.sourceControlsReady&&q('[data-global=scale]')?.value==='100'&&matchMedia('(prefers-reduced-motion: no-preference)').matches")
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
    test('Dropdown entry preserves placement and keyboard ownership', "window.select=q('[data-global=position]');window.trigger=select.nextElementSibling;trigger.focus();key(trigger,'ArrowDown');const menu=q('.precision-select-menu'),a=motion(menu,'dropdown-enter');check(a,'menu entry');check(a.effect.getTiming().duration<=80,'menu entry bounded to 80 ms');check(document.activeElement===trigger,'keyboard focus retained');const r=trigger.getBoundingClientRect(),m=menu.getBoundingClientRect();check(m.left>=6&&m.right<=innerWidth-6,'menu fits horizontally');check(m.top>=r.bottom-2||m.bottom<=r.top+2,'menu placed outside trigger');check(m.top>=0&&m.bottom<=innerHeight,'menu fits vertically');a.pause();a.currentTime=0;check(parseFloat(getComputedStyle(menu).opacity)>=.85,'menu readable at first paint');a.currentTime=50;")
    snap('motion-dropdown-entry')
    test('Keyboard selection commits during entry and removes the menu', "key(trigger,'End');key(trigger,'Enter');check(select.value==='Bottom right','selection committed');check(!q('.precision-select-menu'),'no delayed removal');check(document.activeElement===trigger,'focus retained');trigger.click();check(motion(q('.precision-select-menu'),'dropdown-enter'),'new opening animates');key(trigger,'Escape');check(!q('.precision-select-menu'),'Escape immediate');check(trigger.getAttribute('aria-expanded')==='false','accessible state closed');")
    test('Scroll dismisses a dropdown even during entry', "trigger.click();check(q('.precision-select-menu'),'open');q('.window-content').dispatchEvent(new Event('scroll'));check(!q('.precision-select-menu'),'scroll closes menu');click('[data-action=discard-global]');")
    route('replay')
    test('Filters unfold at shared timing and rapid reversal preserves the latest choice', "window.filters=q('.clip-filter-panel');const summary=filters.querySelector('summary');summary.click();let a=filters.getAnimations()[0];check(a&&a.effect.getTiming().duration===220,'shared duration');a.pause();a.currentTime=100;summary.click();check(filters.querySelector('.clip-filter-fields').inert,'closing content cannot take focus');a=filters.getAnimations()[0];a.pause();a.currentTime=100;summary.click();filters.getAnimations()[0].finish();")
    pump(80)
    test('Settled filter details release temporary dimensions and remain usable', "check(filters.open&&!filters.getAnimations().length,'latest open state');check(filters.style.height===''&&filters.style.overflow==='','layout constraints released');check(!filters.querySelector('.clip-filter-fields').inert,'controls restored');")
    snap('motion-filters-open')
    test('External close cannot be undone by a stale unfolding completion', "filters.querySelector('summary').click();filters.open=false;")
    pump(300)
    test('Externally closed Filters remain closed', "check(!filters.open&&!filters.getAnimations().length,'no late reopening');")
    test('Clip menu unfolds its content without moving the editor header', "window.clipMenu=q('.clip-action-menu');window.headerHeight=clipMenu.getBoundingClientRect().height;clipMenu.querySelector('summary').click();const panel=clipMenu.querySelector('.replay-more-menu');check(panel.getAnimations().length===1,'floating content animates');check(clipMenu.getBoundingClientRect().height===headerHeight,'header stays fixed');panel.getAnimations()[0].finish();")
    pump(80)
    test('Opening compression during menu dismissal leaves no stale panel', "click('[data-action=compress-selected-clip]');check(q('#trim-export-form'),'whole clip form opens');check(!clipMenu.open&&!clipMenu.querySelector('.replay-more-menu').getAnimations().length,'immediate action dismissal');click('#dialog [data-close]');bridge.steamSetup={available:true,configured:true,launch_options:'/usr/bin/redunar-steam-launch --app-id 42 -- %command%'};")
    route('library')
    wait_for("q('.steam-configured-details')")
    test('Detected Steam launch options share the smooth disclosure', "window.steamDetails=q('.steam-configured-details');steamDetails.querySelector('summary').click();const a=steamDetails.getAnimations()[0];check(a&&a.effect.getTiming().duration===220,'shared Steam duration');a.finish();")
    pump(80)
    test('Expanded Steam controls remain intact and do not claim a launch', "check(steamDetails.open&&q('#steam-launch-options').readOnly,'read-only launch option');check(q('[data-copy-steam-options]'),'Copy stays available');check(!bridge.calls.some(c=>c.command==='launch_game'),'no game launched');")
    snap('motion-steam-open')
    test('Library More closes without a late reopen when its action is selected', "const more=q('.game-more');more.querySelector('summary').click();check(more.querySelector(':scope>div').getAnimations().length,'floating Library content animates');click('[data-action=edit-launch]');check(!more.open&&!more.querySelector(':scope>div').getAnimations().length,'action cancels unfolding');check(q('#edit-launch-form'),'action opens its dialog');click('#dialog [data-close]');")
    route('global')
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
    route('replay')
    test('Reduced motion Filters open and close without delayed state', "const details=q('.clip-filter-panel');details.querySelector('summary').click();check(details.open&&!details.getAnimations().length,'immediate open');details.querySelector('summary').click();check(!details.open&&!details.getAnimations().length,'immediate close');")
    test('Reduced motion export busy feedback is a stationary labelled spinner', "const button=document.createElement('button');button.innerHTML='<span class=\"clip-export-spinner\"></span>Exporting…';document.body.append(button);check(!button.querySelector('span').getAnimations().length,'no spinner rotation');check(getComputedStyle(button.querySelector('span')).animationName==='none','stationary spinner');button.remove();")
    pump(300)
    test('Settled workspace has no interaction animation work', "check(!document.getAnimations().some(a=>['dialog-enter','dropdown-enter','save-bar-enter'].includes(a.animationName)),'no residual interaction animation');")
    (ARTIFACTS/'motion-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} motion checks. Screenshots: {ARTIFACTS}')
finally:
    gtk_settings.set_property('gtk-enable-animations', original_animations)
    window.destroy();server.shutdown()
