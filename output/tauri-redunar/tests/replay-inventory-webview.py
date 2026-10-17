"""Completed recordings reach the built UI without restart or player replacement."""
from webview_fixture import *


def add_clip(name, revision):
    js(f"""bridge.clips.unshift({{file_name:{json.dumps(name)},bytes:123456,
        game_name:'ARC Raiders',modified_unix_ns:'1792250000000000000'}});
        bridge.replayRuntime={{phase:'Buffering',can_save:true,completed_save_revision:{revision}}};""")


try:
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#replay')
    wait_for("window.qa&&qa('[data-clip]').length===3")
    pump(1400)
    js("""window.video=q('#clip-video');window.rail=q('.clip-list');
        window.selection=q('[data-selected-clip-title]').textContent;
        window.preparations=bridge.calls.filter(c=>c.command==='clip_playback_path').length;
        // The shared fixture has no playable URL. Supply only the playback
        // state to exercise the production branch without a media dependency.
        Object.defineProperty(video,'paused',{configurable:true,get:()=>false});
        q('#clip-search').value='redunar';q('#clip-search').dispatchEvent(new Event('input',{bubbles:true}));
        q('#clip-search').focus();q('#clip-search').setSelectionRange(2,5);
        q('#save-duration').value='300';q('#save-duration').dispatchEvent(new Event('change',{bubbles:true}));
        window.clipsBefore=bridge.calls.filter(c=>c.command==='replay_clips').length;
    """)
    add_clip('redunar-five-minute.mp4', 2)
    wait_for("qa('[data-clip]').length===4")
    test('Completion paints while another recording plays without replacing the player', """
        check(q('#clip-video')===video&&q('.clip-list')===rail,'player and rail retained');
        check(q('[data-selected-clip-title]').textContent===selection,'selection retained');
        check(q('#clip-search')===document.activeElement&&q('#clip-search').selectionStart===2&&q('#clip-search').selectionEnd===5,'search focus retained');
        check(q('#save-duration').value==='300','five-minute save duration retained');
        check(q('.nav-count').textContent==='4','navigation count updated');
        check([...q('[data-clip-filter=game]').options].some(o=>o.value==='ARC Raiders'),'new game filter available');
        check(bridge.calls.filter(c=>c.command==='clip_playback_path').length===preparations,'no extra playback preparation');
    """)
    snap('replay-inventory-playing')

    # A status failure must not erase the inventory watermark.
    js('bridge.failRuntime=true')
    pump(1400)
    add_clip('redunar-after-status-failure.mp4', 3)
    js('bridge.failRuntime=false')
    wait_for("qa('[data-clip]').length===5")
    test('Status recovery catches up instead of losing the save edge', "check(q('#clip-video')===video,'same player after status recovery')")

    js('bridge.failClips=true')
    add_clip('redunar-after-inventory-failure.mp4', 4)
    wait_for("bridge.calls.filter(c=>c.command==='replay_clips').length>clipsBefore+2")
    test('Failed inventory preserves the previous visible recordings and selection', "check(qa('[data-clip]').length===5&&q('#clip-video')===video,'known recordings retained');check(q('.clip-list [role=status]').textContent.includes('Fixture inventory unavailable'),'failed refresh explained')")
    js('bridge.failClips=false')
    wait_for("qa('[data-clip]').length===6")
    pump(1200)
    js("window.readsAfterRecovery=bridge.calls.filter(c=>c.command==='replay_clips').length")
    pump(1300)
    test('Recovered inventory is acknowledged once', "check(bridge.calls.filter(c=>c.command==='replay_clips').length===readsAfterRecovery,'no repeated inventory reads')")

    # A pending inventory snapshot cannot acknowledge a later committed save.
    js('bridge.holdClips=true')
    add_clip('redunar-in-snapshot.mp4', 5)
    wait_for('bridge.releaseClips')
    js("""click('[data-clip="redunar-after-status-failure.mp4"]');
        window.video=q('#clip-video');window.selection=q('[data-selected-clip-title]').textContent;
        Object.defineProperty(video,'paused',{configurable:true,get:()=>false});""")
    add_clip('redunar-after-snapshot.mp4', 6)
    js('bridge.holdClips=false;bridge.releaseClips();delete bridge.releaseClips')
    wait_for("qa('[data-clip]').length===8")
    test('Save and selection during an inventory read survive the pending snapshot', "check(q('#clip-video')===video&&q('[data-selected-clip-title]').textContent===selection,'pending read preserved latest selection')")

    js("Object.defineProperty(video,'paused',{configurable:true,get:()=>true});bridge.idle=true")
    add_clip('redunar-after-game-exit.mp4', 7)
    js("bridge.replayRuntime.phase='Inactive';bridge.replayRuntime.can_save=false")
    wait_for("qa('[data-clip]').length===9")
    test('Completion after game exit updates a paused player in place', "check(q('#clip-video')===video&&q('#save-duration').value==='300','paused player and duration retained');check(!bridge.errors.length,bridge.errors.join('; '))")
    snap('replay-inventory-after-game-exit')

    js("""Object.defineProperty(document,'hidden',{configurable:true,get:()=>bridge.hidden===true});
        bridge.hidden=true;window.hiddenReads=bridge.calls.filter(c=>c.command==='replay_clips').length;""")
    add_clip('redunar-while-hidden.mp4', 8)
    pump(2200)
    test('Hidden windows defer inventory work', "check(bridge.calls.filter(c=>c.command==='replay_clips').length===hiddenReads,'no hidden inventory polling');bridge.hidden=false")
    wait_for("qa('[data-clip]').length===10")
    test('Returning to the window catches up without restarting', "check(q('#clip-video')===video,'hidden-window catch-up retained player')")

    js("""bridge.clips.push(...Array.from({length:20},(_,i)=>({file_name:`redunar-scroll-${i}.mp4`,bytes:100,
        modified_unix_ns:'1792250000000000000'})));click('[data-action=reload-clips]');""")
    wait_for("qa('[data-clip]').length===30")
    js("""const cards=qa('[data-clip]');cards[15].focus({preventScroll:true});
        rail.scrollTop=400;window.railTop=rail.scrollTop;window.focusedClip=document.activeElement.dataset.clip;
        check(railTop>0,'scroll fixture has overflow');""")
    pump(250)
    js("check(rail.scrollTop===railTop,'fixture settled at requested scroll: '+rail.scrollTop)")
    add_clip('redunar-scroll-retained.mp4', 9)
    wait_for("qa('[data-clip]').length===31")
    test('Completion preserves rail scroll and a focused clip card', "check(rail.scrollTop===railTop,'rail scroll retained: expected '+railTop+', got '+rail.scrollTop);check(document.activeElement.dataset.clip===focusedClip,'card focus retained');check(q('#clip-video')===video,'manual and automatic refresh retained paused player')")
    js("window.sortTrigger=q('[data-clip-filter=sort]').nextElementSibling;sortTrigger.focus()")
    add_clip('redunar-sort-focus.mp4', 10)
    wait_for("qa('[data-clip]').length===32")
    test('Completion retains sort focus and its current value', "check(document.activeElement===sortTrigger,'sort control retained');check(q('[data-clip-filter=sort]').value==='newest','sort preserved');check(!bridge.errors.length,bridge.errors.join('; '))")
    (ARTIFACTS/'replay-inventory-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(json.dumps({'checks':len(checks),'artifacts':str(ARTIFACTS)}))
finally:
    window.destroy()
    server.shutdown()
