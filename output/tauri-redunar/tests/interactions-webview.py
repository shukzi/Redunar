"""Refresh, toggles and native scrollbar input with isolated command fixtures."""
import webview_fixture as harness
from webview_fixture import *
ARTIFACTS=NATIVE.parents[1]/'.redunar-build/reports/interaction-ui';ARTIFACTS.mkdir(parents=True,exist_ok=True);harness.ARTIFACTS=ARTIFACTS
extra=r"""
bridge.startupUpdateAvailable=false;bridge.automaticUpdates=false;bridge.idle=true;
bridge.historyFixture=Array.from({length:18},(_,i)=>({id:i+1,game:'Session '+(i+1),started_unix:1789300000-i*86400,duration_seconds:8640,average_fps:67,timeline:[]}));
const originalInvoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(['start_microphone_test','microphone_test_status','stop_microphone_test'].includes(command)){
  bridge.calls.push({command,args});
  if(command==='stop_microphone_test'){bridge.testRunning=false;return;}
  bridge.testRunning=true;return {id:91,phase:'recording',seconds_recorded:0,levels:{peak:.2,rms:.1,limiting:false}};
 }

 if(command==='session_history'){
  bridge.historyReads=(bridge.historyReads||0)+1;
  if(bridge.holdHistory)await new Promise(resolve=>bridge.releaseHistory=resolve);
  if(bridge.failHistory)throw Error('Fixture journal read failed: EACCES');
 }
 if(command==='microphone_inputs'&&bridge.holdInputs)await new Promise(resolve=>bridge.releaseInputs=resolve);
 if(command==='set_close_to_tray'&&bridge.holdPreference)await new Promise(resolve=>bridge.releasePreference=resolve);
 return originalInvoke(command,args);
};
window.movedToggle=selector=>new DOMMatrix(getComputedStyle(q(selector).nextElementSibling,'::before').transform).e;
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
original_animations=Gtk.Settings.get_default().get_property('gtk-enable-animations')
Gtk.Settings.get_default().set_property('gtk-enable-animations',True)
def pointer(kind,x,y,state=0,delay=50):
 e=Gdk.Event.new(kind);e.window=view.get_window();e.send_event=True;e.time=Gdk.CURRENT_TIME;e.set_device(Gdk.Display.get_default().get_default_seat().get_pointer());e.x=x;e.y=y;e.x_root=x;e.y_root=y;e.state=state
 if kind in (Gdk.EventType.BUTTON_PRESS,Gdk.EventType.BUTTON_RELEASE):e.button=1
 if kind==Gdk.EventType.MOTION_NOTIFY:e.is_hint=False
 view.event(e);pump(delay)
def center(selector,label=False):
 return json.loads(js("const el=q("+json.dumps(selector)+")"+(".closest('.switch')" if label else "")+";el.scrollIntoView({block:'center'});const b=el.getBoundingClientRect();return JSON.stringify({x:b.x+b.width/2,y:b.y+b.height/2})"))
try:
 view.load_uri(f'http://127.0.0.1:{server.server_port}/#history')
 wait_for("typeof q==='function'&&q('[data-action=reload-history]')&&!q('[data-action=reload-history]').disabled")
 pump(400)
 point=json.loads(js("const b=q('#session-list').getBoundingClientRect();return JSON.stringify({x:b.right-7,y:b.top+20})"))
 pointer(Gdk.EventType.MOTION_NOTIFY,point['x'],point['y'],delay=200)
 pointer(Gdk.EventType.BUTTON_PRESS,point['x'],point['y'],delay=200)
 snap('interaction-scrollbar-pressed')
 pixel=GdkPixbuf.Pixbuf.new_from_file(str(ARTIFACTS/'interaction-scrollbar-pressed.png'))
 x=round(point['x']*pixel.get_width()/view.get_allocated_width());y=round(point['y']*pixel.get_height()/view.get_allocated_height())
 start=y*pixel.get_rowstride()+x*pixel.get_n_channels();color=list(pixel.get_pixels()[start:start+3])
 js('window.pressedScrollbarColor='+json.dumps(color))
 test('Native pressed scrollbar retains a visible neutral thumb',"const c=pressedScrollbarColor;check(Math.max(...c)-Math.min(...c)<=3&&Math.max(...c)>40,'pressed thumb is not black: '+c);")
 pointer(Gdk.EventType.MOTION_NOTIFY,point['x'],point['y']+120,Gdk.ModifierType.BUTTON1_MASK,delay=150)
 test('Native scrollbar drag scrolls the session list',"check(q('#session-list').scrollTop>0,'platform scrollbar drag');")
 snap('interaction-scrollbar-dragged')
 pointer(Gdk.EventType.BUTTON_RELEASE,point['x'],point['y']+120,Gdk.ModifierType.BUTTON1_MASK)
 js("q('#session-list').scrollTop=0")
 js("qa('[data-session]')[1].click();q('#session-list').scrollTop=160;bridge.holdHistory=true;window.reads=bridge.historyReads;q('[data-action=reload-history]').focus();click('[data-action=reload-history]');")
 wait_for("bridge.releaseHistory&&q('[data-refresh-key=history]').getAttribute('aria-busy')==='true'")
 test('History refresh shows native-read progress and rejects duplicates',"const b=q('[data-refresh-key=history]');check(b.disabled&&b.dataset.refreshState==='pending','pending control');check(b.querySelector('svg').getAnimations().length>0,'progress motion');b.dispatchEvent(new Event('click',{bubbles:true}));check(bridge.historyReads===reads+1,'one request');")
 snap('history-refresh-pending')
 js("bridge.historyFixture.unshift({id:99,game:'New session',started_unix:1789300010,duration_seconds:100,average_fps:120,timeline:[]});bridge.holdHistory=false;bridge.releaseHistory();")
 wait_for("q('[data-refresh-key=history]')?.dataset.refreshState==='success'")
 test('Refresh acknowledges completion and retains session and scroll position',"check(q('.history-summary h2').textContent==='Session 2','selected record survives reorder');check(q('#session-list').scrollTop===160,'rail position retained');check(q('[data-refresh-key=history] [data-lucide=check]'),'confirmed read');check(document.activeElement===q('[data-refresh-key=history]'),'refresh focus restored');")
 snap('history-refresh-success')
 pump(1650)
 js("bridge.holdHistory=true;click('[data-action=reload-history]');qa('[data-session]')[3].click();qa('[data-session]')[3].focus();bridge.holdHistory=false;bridge.releaseHistory();")
 wait_for("q('[data-refresh-key=history]')?.dataset.refreshState==='success'")
 test('A pending refresh respects a newer session choice and its focus',"check(q('.history-summary h2').textContent==='Session 3','newer selected record retained');check(document.activeElement.matches('[data-session][aria-pressed=true]'),'row focus retained');")
 js("bridge.failHistory=true;click('[data-action=reload-history]')")
 wait_for("q('[data-refresh-key=history]')?.dataset.refreshState==='error'")
 test('Failed refresh keeps technical feedback and permits retry',"check(q('.history-empty').textContent.includes('EACCES'),'real error retained');check(!q('[data-refresh-key=history]').disabled,'retry available');bridge.failHistory=false;click('[data-action=reload-history]');")
 wait_for("q('[data-refresh-key=history]')?.dataset.refreshState==='success'")
 pump(1650)
 js("bridge.holdHistory=true;click('[data-action=reload-history]')")
 wait_for('typeof bridge.releaseHistory==="function"')
 route('global');js("window.keepScale=q('[data-global=scale]');keepScale.value='105';keepScale.dispatchEvent(new Event('input',{bubbles:true}));bridge.holdHistory=false;bridge.releaseHistory();")
 pump(400)
 test('Late history feedback leaves another page and its draft mounted',"check(q('[data-global=scale]')===keepScale&&keepScale.value==='105','draft node retained');")
 test('Overlay help stays stable through ordinary edits and preset changes',"window.overlayHelp=q('.overlay-visibility-description').textContent;window.brandingHelp=q('[data-global=branding]').closest('.field').querySelector('p').textContent;window.metricHelp=q('.metric-hint').textContent;q('[data-global=overlay]').click();check(q('.overlay-visibility-description').textContent===overlayHelp,'toggle keeps explanation');check(q('#hud').hidden,'preview still follows value');click('[data-action=discard-global]');click('[data-overlay-editor-tab=metrics]');click('[data-preset=Custom]');check(q('.metric-hint').textContent===metricHelp,'Custom keeps explanation');check(!q('[data-metric]').disabled,'Custom unlocks real metrics');click('[data-preset=\"FPS only\"]');check(q('[data-metric]').disabled&&q('[data-global=branding]').disabled,'FPS only locks ignored controls');check(q('.metric-hint').textContent===metricHelp&&q('[data-global=branding]').closest('.field').querySelector('p').textContent===brandingHelp,'preset keeps explanations');check(q('.overlay-layout-hint').hidden,'no extra description appears');click('[data-action=discard-global]');click('[data-overlay-editor-tab=appearance]');")
 selector='[data-global=overlay]';point=center(selector,True);pump(300)
 pointer(Gdk.EventType.MOTION_NOTIFY,point['x'],point['y'])
 pointer(Gdk.EventType.BUTTON_PRESS,point['x'],point['y'],delay=90)
 test('Toggle press has a restrained track and handle response',"const s=q('[data-global=overlay]').nextElementSibling;check(new DOMMatrix(getComputedStyle(s).transform).a<1,'track presses');check(new DOMMatrix(getComputedStyle(s,'::before').transform).a>1,'handle widens');")
 snap('toggle-pressed')
 pointer(Gdk.EventType.BUTTON_RELEASE,point['x'],point['y'],Gdk.ModifierType.BUTTON1_MASK,delay=50)
 test('Toggle release glides between its two real endpoints',"const x=movedToggle('[data-global=overlay]');check(x>0&&x<18,'intermediate slide position '+x);check(!q('.settings-save-bar').hidden,'draft preserved');")
 pump(350);js("click('[data-action=discard-global]')")
 point=json.loads(js("const r=q('[data-global=scale]'),b=r.getBoundingClientRect(),ratio=(Number(r.value)-Number(r.min))/(Number(r.max)-Number(r.min));return JSON.stringify({x:b.left+9+ratio*(b.width-18),y:b.top+b.height/2,left:b.left,top:b.top})"))
 pointer(Gdk.EventType.MOTION_NOTIFY,point['left']-20,point['top']-10,delay=240);snap('level-slider-idle')
 pointer(Gdk.EventType.MOTION_NOTIFY,point['x'],point['y'],delay=240);snap('level-slider-hover')
 pointer(Gdk.EventType.BUTTON_PRESS,point['x'],point['y'],delay=120);snap('level-slider-pressed')
 test('Level slider press retains its exact-value owner',"const r=q('[data-global=scale]');check(q('#scale-value').value===r.value,'exact value stays aligned');window.rangeBefore=Number(r.value);")
 pointer(Gdk.EventType.MOTION_NOTIFY,point['x']+60,point['y'],Gdk.ModifierType.BUTTON1_MASK,delay=50)
 test('Level dragging updates values and fill directly without saving a draft',"const r=q('[data-global=scale]'),v=Number(r.value),fill=100*(v-Number(r.min))/(Number(r.max)-Number(r.min));check(v>rangeBefore&&v<=Number(r.max),'native drag changes value within bounds');check(q('#scale-value').value===r.value,'value follows drag');check(Math.abs(parseFloat(r.style.getPropertyValue('--range-fill'))-fill)<.01,'fill follows value');check(!bridge.calls.some(c=>c.command==='save_global_settings'),'no implicit save');")
 pointer(Gdk.EventType.BUTTON_RELEASE,point['x']+60,point['y'],Gdk.ModifierType.BUTTON1_MASK)
 js("click('[data-action=discard-global]')")
 route('settings');js("bridge.holdPreference=true;window.tray=q('[data-preference=tray]');tray.click()")
 wait_for("bridge.releasePreference&&tray.getAttribute('aria-busy')==='true'")
 test('A saving toggle keeps its saved position and remains visibly busy',"check(q('[data-preference=tray]')===tray&&!tray.checked&&tray.disabled,'acknowledged state retained');check(getComputedStyle(tray.closest('.switch')).opacity==='1','pending does not look permanently disabled');bridge.holdPreference=false;bridge.releasePreference();")
 wait_for("tray.checked&&!tray.disabled")
 test('Acknowledgement retains the original toggle and starts its slide',"check(q('[data-preference=tray]')===tray,'toggle never remounted');")
 pump(300);js("window.log=q('[data-preference=diagnostic-log]');log.click()")
 wait_for("log.checked&&!log.disabled")
 test('Debug log saves in place and updates its restart guidance',"check(q('[data-preference=diagnostic-log]')===log,'log toggle retained');check(!q('#diagnostic-log-status').hidden&&q('#diagnostic-log-status').textContent.includes('Restart'),'status updates without remount');")
 route('global');js("click('[data-global-tab=replay]')")
 wait_for("q('[data-microphone-source]')&&q('[data-action=reload-microphone-devices]')?.dataset.refreshState==='ready'&&!q('.microphone-device-status')")
 test('Microphone-off controls are disabled in both behavior and appearance',"const select=q('[data-microphone-source]'),refresh=q('[data-refresh-key=inputs]'),trigger=select.nextElementSibling;check(select.disabled&&trigger.disabled&&refresh.disabled,'both input controls disabled');check(Number(getComputedStyle(trigger).opacity)<.5&&Number(getComputedStyle(refresh).opacity)<.5,'clearly dimmed');window.inputCalls=bridge.calls.filter(c=>c.command==='microphone_inputs').length;refresh.click();select.value='usb-mic';select.dispatchEvent(new Event('change',{bubbles:true}));check(!bridge.calls.some(c=>c.command==='set_replay_microphone'),'no disabled input write');check(bridge.calls.filter(c=>c.command==='microphone_inputs').length===inputCalls,'no disabled refresh');check(q('#microphone-input-help').hidden,'off description stays hidden');window.inputHelp=q('#microphone-input-help').textContent;window.mic=q('[data-replay-preference=microphone]');mic.click();")
 wait_for("mic.checked&&!mic.disabled&&!q('[data-refresh-key=inputs]').disabled")
 test('Enabling the microphone retains its toggle and stable input guidance',"check(q('[data-replay-preference=microphone]')===mic,'mic toggle retained');check(q('.microphone-device>div>p').hidden&&q('#microphone-input-help').textContent===inputHelp,'no description change');check(!q('[data-microphone-source]').disabled,'input available');bridge.holdInputs=true;window.refresh=q('[data-refresh-key=inputs]');window.refreshWidth=refresh.getBoundingClientRect().width;refresh.click();")
 wait_for("bridge.releaseInputs&&q('[data-refresh-key=inputs]').dataset.refreshState==='pending'")
 test('Input refresh has visible progress without shifting its field',"check(refresh.disabled&&getComputedStyle(refresh).opacity==='1','progress visible');check(refresh.getBoundingClientRect().width===refreshWidth,'stable width');check(q('[data-microphone-source]').disabled,'choice waits for discovery');bridge.holdInputs=false;bridge.releaseInputs();")
 wait_for("q('[data-refresh-key=inputs]').dataset.refreshState==='success'")
 test('Input refresh confirms completion and restores the saved choices',"check(q('[data-refresh-key=inputs]')===refresh&&refresh.querySelector('.refresh-label').textContent==='Refresh inputs'&&refresh.querySelector('.refresh-status').textContent==='Inputs refreshed','in-place acknowledgement');check(!q('[data-microphone-source]').disabled,'choice enabled');check(refresh.getBoundingClientRect().width===refreshWidth,'confirmation width stable');")
 js("q('.microphone-device').scrollIntoView({block:'center'})");snap('microphone-controls-enabled')
 pump(1650);js("bridge.failInputs=true;refresh.click()")
 wait_for("q('[data-refresh-key=inputs]').dataset.refreshState==='error'")
 test('Input discovery failures remain separate and recoverable',"check(q('.microphone-device-status').textContent.includes('Fixture audio server unavailable'),'technical input error');check(!refresh.disabled,'retry available');bridge.failInputs=false;refresh.click();")
 wait_for("q('[data-refresh-key=inputs]').dataset.refreshState==='success'")
 js("click('[data-microphone-test]')");wait_for('bridge.testRunning===true')
 js('mic.click()');wait_for('!mic.checked&&!mic.disabled')
 test('Disabling the microphone restores disabled controls and cancels the explicit test',"check(q('[data-refresh-key=inputs]').disabled&&q('[data-microphone-source]').disabled,'controls disabled again');check(bridge.testRunning===false&&bridge.calls.filter(c=>c.command==='stop_microphone_test').length===1,'test capture stops once');")
 pump(350);js("q('.microphone-device').scrollIntoView({block:'center'})");snap('microphone-controls-disabled')
 route('library')
 test('Game overrides retain their explanation while inheritance and drafts stay accurate',"window.banner=q('.inheritance-banner').textContent;window.gameHelp=q('[data-override=overlay]').closest('.field').querySelector('p').textContent;const control=q('[data-override=overlay]').closest('.choice-switcher');control.querySelector('[data-choice-index=\"2\"]').click();check(q('.inheritance-banner').textContent===banner,'banner stable');check(q('[data-override=overlay]').closest('.field').querySelector('p').textContent===gameHelp,'description stable');check(q('[data-override=overlay]').value==='Off'&&q('.custom-setting'),'custom value remains explicit');check(!q('.game-save-bar').hidden,'draft still requires Save');click('[data-action=discard-game]');check(q('[data-override=overlay]').value==='inherit'&&q('.game-save-bar').hidden,'Discard restores inheritance');check(q('.inheritance-banner').textContent===banner,'revert keeps explanation');")
 route('global')
 Gtk.Settings.get_default().set_property('gtk-enable-animations',False)
 # Match the existing motion fixture: evaluate CSS afresh under this GTK setting.
 view.reload();pump(500)
 wait_for("typeof q==='function'&&q('[data-global=overlay]')&&matchMedia('(prefers-reduced-motion: reduce)').matches")
 route('global');js("click('[data-global-tab=overlay]');q('[data-global=overlay]').click()")
 test('Reduced motion keeps toggle state immediate',"const s=q('[data-global=overlay]').nextElementSibling;check(getComputedStyle(s,'::before').transitionDuration==='0s','no toggle slide');")
 route('history');js("bridge.holdHistory=true;click('[data-action=reload-history]')")
 test('Reduced motion refresh progress remains labelled and stationary',"const b=q('[data-refresh-key=history]');check(b.getAttribute('aria-busy')==='true','busy label retained');check(!b.querySelector('svg').getAnimations().length,'no rotation');bridge.holdHistory=false;bridge.releaseHistory();check(bridge.errors.length===0,bridge.errors.join('; '));")
 (ARTIFACTS/'interaction-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
 print(f'PASS: {len(checks)} interaction checks with private platform pointer input.',flush=True)
finally:
 Gtk.Settings.get_default().set_property('gtk-enable-animations',original_animations)
 window.destroy();server.shutdown()
