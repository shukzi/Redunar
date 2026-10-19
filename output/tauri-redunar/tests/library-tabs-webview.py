"""Library tabs retain Steam evidence and drafts in isolated production WebKit."""
from webview_fixture import *

extra = r"""
bridge.idle=true;bridge.automaticUpdates=false;
bridge.catalog=structuredClone(games);
bridge.catalog.push({...structuredClone(games[0]),id:'3',name:'Another Steam game',steam_app_id:730});
bridge.holdSetup=true;bridge.setupRequests=[];
const invoke=window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
 if(command==='save_game_profile'){if(bridge.failGameProfile)throw Error('Fixture game profile conflict');const result=await invoke(command,args);return {...result,games:[...result.games,bridge.catalog.find(g=>g.id==='3')]};}
 if(command!=='steam_setup_status')return invoke(command,args);
 const request={gameId:args.gameId};bridge.setupRequests.push(request);
 if(bridge.holdSetup)await new Promise(resolve=>request.release=resolve);
 const setup=await invoke(command,args),appId=args.gameId==='3'?730:42;
 return {...setup,app_id:appId,launch_options:`/usr/bin/redunar-steam-launch --app-id ${appId} -- %command%`};
};
window.nativeReads=()=>JSON.stringify(bridge.calls.filter(c=>['game_installation_statuses','game_banner','game_poster'].includes(c.command)));
window.override=(key,value)=>{const s=q('[data-override='+key+']');s.value=value;s.dispatchEvent(new Event('change',{bubbles:true}));};
window.tabs=()=>{click('[data-library-tab=match]');click('[data-library-tab=profile]');};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#library')
    pump(500)
    wait_for("q('.steam-setup-card')&&bridge.setupRequests.length===1&&q('[data-override=overlay]')")
    test('Pending Steam check survives rapid tab changes without duplicate reads', "window.panel=q('.steam-setup-card');window.content=q('[data-steam-setup-content]');window.checking=content.firstElementChild;window.search=q('#game-search');window.banner=q('[data-banner]');window.reads=nativeReads();tabs();tabs();check(q('.steam-setup-card')===panel&&content.firstElementChild===checking,'same pending panel');check(bridge.setupRequests.length===1,'only original request');check(nativeReads()===reads,'no installation or artwork reads');check(q('#game-search')===search&&q('[data-banner]')===banner,'surrounding view retained');override('overlay','Off');check(q('.steam-setup-card')===panel&&content.firstElementChild===checking,'pending Steam retained during edit');check(bridge.setupRequests.length===1,'edit did not recheck');click('[data-action=discard-game]');check(q('.steam-setup-card')===panel&&q('[data-override=overlay]').value==='inherit','discard retains pending Steam');override('overlay','Off');click('[data-action=reset-game]');check(q('.steam-setup-card')===panel&&bridge.setupRequests.length===1,'reset retains pending Steam');bridge.setupRequests[0].release();")
    wait_for("q('#steam-launch-options')")
    test('Pending result reaches the retained panel after a tab switch', "check(q('.steam-setup-card')===panel,'result retained panel');check(q('#steam-launch-options').value.includes('--app-id 42'),'correct game command');window.body=q('.game-detail > .detail-body');click('[data-library-tab=profile]');check(q('.game-detail > .detail-body')===body,'active tab is a no-op');")
    js("bridge.holdSetup=false;bridge.steamSetup={available:true,configured:true,configuration_state:'configured'};window.dispatchEvent(new Event('focus'));")
    wait_for("q('.steam-configured-details')")
    test('Focus still refreshes current Steam evidence', "check(bridge.setupRequests.length===2,'focus rechecked');check(q('.steam-setup-card')===panel,'focus uses same panel');const s=q('[data-override=overlay]');s.value='Off';s.dispatchEvent(new Event('change',{bubbles:true}));")
    pump(300)
    test('Tab changes preserve expanded command, selection, search and unsaved actions', "window.panel=q('.steam-setup-card');window.options=q('#steam-launch-options');window.details=q('.steam-configured-details');details.open=true;options.setSelectionRange(4,15);window.search=q('#game-search');search.value='Test';search.dispatchEvent(new Event('input',{bubbles:true}));window.bar=q('.game-save-bar');window.reads=nativeReads();window.requests=bridge.setupRequests.length;tabs();check(q('.steam-setup-card')===panel&&q('#steam-launch-options')===options,'command retained');check(details.open&&options.selectionStart===4&&options.selectionEnd===15,'expansion and selection retained');check(q('#game-search')===search&&search.value==='Test','search retained');check(q('.game-save-bar')===bar&&!bar.hidden&&!q('[data-action=save-game]').disabled,'draft actions retained');check(q('[data-override=overlay]').value==='Off','draft retained');check(nativeReads()===reads&&bridge.setupRequests.length===requests,'no native reads');")
    test('Per-game edits, reset and discard keep expanded Steam evidence mounted', "window.requests=bridge.setupRequests.length;override('captureMetrics','Off');check(q('.steam-setup-card')===panel&&q('#steam-launch-options')===options&&details.open,'edit retains command and expansion');check(options.selectionStart===4&&options.selectionEnd===15,'selection retained');check(q('[data-override=overlay]').value==='Off'&&q('[data-override=captureMetrics]').value==='Off','both drafts retained');check(document.activeElement===q('[data-override=captureMetrics]').nextElementSibling,'edited control focus restored');check(q('.library-game.selected [data-profile-status]').textContent.startsWith('Custom settings'),'catalog badge updated');click('[data-action=reset-game]');check(q('[data-override=overlay]').value==='inherit'&&q('[data-override=captureMetrics]').value==='inherit','reset inherited draft');check(q('.library-game.selected [data-profile-status]').hidden,'catalog badge reset');override('overlay','Off');check(q('.steam-setup-card')===panel&&q('#steam-launch-options')===options&&details.open,'reset retains panel');check(bridge.setupRequests.length===requests&&nativeReads()===reads,'no extra native reads');")
    pump(200)
    test('Tabs retain focus and do not restart draft entry', "const tab=q('[data-library-tab=match]');tab.focus();tab.click();check(document.activeElement===tab,'tab focus retained');check(tab.getAttribute('aria-pressed')==='true','selected state updated');check(!bar.getAnimations().length,'no repeated bar animation');click('[data-library-tab=profile]');click('[data-action=discard-game]');check(q('[data-override=overlay]').value==='inherit'&&q('.game-save-bar').hidden,'discard restores inheritance');")
    js("override('overlay','Off');bridge.failGameProfile=true;click('[data-action=save-game]')")
    wait_for("q('#toast').textContent.includes('Fixture game profile conflict')&&!q('[data-action=save-game]').disabled")
    test('Failed per-game save retains draft, retry and Steam command', "check(q('.steam-setup-card')===panel&&q('#steam-launch-options')===options&&details.open,'failure retains panel');check(q('[data-override=overlay]').value==='Off'&&!q('.game-save-bar').hidden,'failed draft retained');check(bridge.setupRequests.length===requests&&nativeReads()===reads,'failure no recheck');bridge.failGameProfile=false;click('[data-action=save-game]');")
    wait_for("q('.game-save-bar').hidden")
    test('Successful per-game save retains Steam evidence and committed inheritance state', "check(q('.steam-setup-card')===panel&&q('#steam-launch-options')===options&&details.open,'save retains panel');check(q('[data-override=overlay]').value==='Off','saved override remains');check(bridge.setupRequests.length===requests&&nativeReads()===reads,'save no extra native reads');override('overlay','inherit');click('[data-action=discard-game]');check(q('[data-override=overlay]').value==='Off','discard returns to committed value');check(q('.steam-setup-card')===panel,'discard retains panel');")
    pump(300)
    js("bridge.failSteamSetup=true;window.dispatchEvent(new Event('focus'));")
    wait_for("q('.steam-setup-retry')&&!q('.steam-setup-retry').disabled")
    test('Failure evidence remains visible across tabs', "window.panel=q('.steam-setup-card');window.retry=q('.steam-setup-retry');window.requests=bridge.setupRequests.length;tabs();check(q('.steam-setup-card')===panel&&q('.steam-setup-retry')===retry,'error and retry retained');check(panel.textContent.includes('could not be checked'),'honest error retained');check(bridge.setupRequests.length===requests,'no implicit retry');bridge.holdSetup=true;retry.click();check(retry.disabled,'retry pending');tabs();check(q('.steam-setup-retry')===retry&&retry.disabled,'pending retry retained');check(bridge.setupRequests.length===requests+1,'one retry');bridge.failSteamSetup=false;bridge.setupRequests.at(-1).release();")
    wait_for("q('.steam-configured-details')")
    snap('library-tabs-steam-ready')
    test('Late checks cannot overwrite another selected Steam game', "bridge.holdSetup=true;window.dispatchEvent(new Event('focus'));window.oldRequest=bridge.setupRequests.at(-1);q('#game-search').value='';q('#game-search').dispatchEvent(new Event('input',{bubbles:true}));click('[data-game=\"3\"]');window.newRequest=bridge.setupRequests.at(-1);check(newRequest!==oldRequest&&newRequest.gameId==='3','fresh game request');tabs();newRequest.release();")
    wait_for("q('#steam-launch-options')?.value.includes('--app-id 730')")
    js("oldRequest.release()")
    pump(200)
    test('Out-of-order completion keeps the selected game command', "check(q('#steam-launch-options').value.includes('--app-id 730'),'late old result ignored');bridge.holdSetup=false;bridge.steamSetup={available:true,configured:false,configuration_state:'ambiguous',status:'Review the local Steam accounts'};window.dispatchEvent(new Event('focus'));")
    wait_for("q('.steam-setup-notice')")
    test('Ambiguity stays visible and direct games omit Steam setup', "tabs();check(q('.steam-setup-notice').textContent.includes('Review the local Steam accounts'),'ambiguity retained');click('[data-game=\"2\"]');tabs();check(!q('.steam-setup-card'),'no Steam evidence for direct game');check(q('[data-override=overlay]').value==='Off','direct custom profile retained');")
    window.resize(640,800);pump()
    snap('library-tabs-direct-narrow')
    test('Narrow tab content fits without script errors', "check(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow');check(bridge.errors.length===0,bridge.errors.join('; '));")
    (ARTIFACTS/'library-tabs-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} Library tab checks. Screenshots: {ARTIFACTS}')
finally:
    window.destroy();server.shutdown()
