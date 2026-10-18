"""Completed recordings reach the built UI without restart or player replacement."""
from webview_fixture import *

gtk_settings = Gtk.Settings.get_default()
original_animations = gtk_settings.get_property('gtk-enable-animations')
gtk_settings.set_property('gtk-enable-animations', True)
extra = r"""
bridge.clipMotion=[];
const originalAnimate=Element.prototype.animate;
Element.prototype.animate=function(frames,timing){
 bridge.clipMotion.push({clip:this.querySelector('[data-clip]')?.dataset.clip,
  thumbnail:this.tagName==='IMG',frames,duration:timing.duration});
 return originalAnimate.call(this,frames,timing);
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))


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
        bridge.clipMotion=[];
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
    wait_for("bridge.clipMotion.some(m=>m.thumbnail)")
    test('Only the arriving clip fades while decoded thumbnails reveal separately', """
        const arrivals=bridge.clipMotion.filter(m=>m.clip&&m.frames[0].opacity===0);
        check(arrivals.length===1&&arrivals[0].clip==='redunar-five-minute.mp4','one new recording fades');
        check(arrivals[0].duration===180,'short arrival fade');
        check(bridge.clipMotion.some(m=>m.thumbnail&&m.duration===160),'decoded image fade');
        bridge.clipMotion=[];q('#clip-search').dispatchEvent(new Event('input',{bubbles:true}));
        check(!bridge.clipMotion.length,'filter redraw never replays arrivals');
    """)

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
    js("bridge.brokenThumbnail=true;window.thumbnailReads=bridge.calls.filter(c=>c.command==='clip_thumbnail').length;rail.scrollTop=0;rail.dispatchEvent(new Event('scroll'))")
    add_clip('redunar-broken-thumbnail.mp4', 11)
    wait_for("qa('[data-clip]').length===33")
    wait_for("bridge.calls.filter(c=>c.command==='clip_thumbnail').length>thumbnailReads")
    pump(500)
    test('Failed image decode retains the usable thumbnail placeholder', "const card=q('[data-clip=\"redunar-broken-thumbnail.mp4\"]');check(card.querySelector('.thumbnail>.icon'),'placeholder retained');check(!card.querySelector('img'),'failed image removed');check(q('#clip-video')===video,'decode failure preserves playback');bridge.brokenThumbnail=false")
    test('Filter redraws retain the failed-thumbnail placeholder', "q('#clip-search').dispatchEvent(new Event('input',{bubbles:true}));const card=q('[data-clip=\"redunar-broken-thumbnail.mp4\"]');check(card.querySelector('.thumbnail>.icon')&&!card.querySelector('img'),'unavailable image remains a placeholder');")
    gtk_settings.set_property('gtk-enable-animations', False)
    pump(300)
    wait_for("matchMedia('(prefers-reduced-motion: reduce)').matches")
    js('bridge.clipMotion=[]')
    add_clip('redunar-reduced-motion.mp4', 12)
    wait_for("qa('[data-clip]').length===34")
    wait_for("q('[data-clip=\"redunar-reduced-motion.mp4\"] img')?.naturalWidth>0")
    test('Reduced motion presents arrivals and thumbnails immediately', "check(!bridge.clipMotion.length,'no arrival, layout or thumbnail animations: '+JSON.stringify(bridge.clipMotion));check(q('[data-clip=\"redunar-reduced-motion.mp4\"] img'),'decoded thumbnail visible');check(!document.getAnimations().length,'no residual animation');")
    gtk_settings.set_property('gtk-enable-animations', True)
    pump(300)
    wait_for("matchMedia('(prefers-reduced-motion: no-preference)').matches")
    js("bridge.clips=[];click('[data-action=reload-clips]')")
    wait_for("qa('[data-clip]').length===0&&!q('#clip-video')")
    js('bridge.clipMotion=[]')
    add_clip('redunar-first-after-empty.mp4', 13)
    wait_for("qa('[data-clip]').length===1")
    test('The first arrival into an empty collection also gets the approved fade', "check(bridge.clipMotion.some(m=>m.clip==='redunar-first-after-empty.mp4'&&m.duration===180),'empty-list arrival fades');")
    (ARTIFACTS/'replay-inventory-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(json.dumps({'checks':len(checks),'artifacts':str(ARTIFACTS)}))
finally:
    gtk_settings.set_property('gtk-enable-animations', original_animations)
    window.destroy()
    server.shutdown()
