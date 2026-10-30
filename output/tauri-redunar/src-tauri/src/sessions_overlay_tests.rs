// Included in the session test module to reuse its isolated child fixture.
#[test]
fn saved_visibility_resolves_inheritance_for_only_the_running_game() {
    use redunar_core::Inheritable;
    let mut f = Fixture::new();
    let service = f.engine.service.clone();
    let mut global = GlobalGameProfile {
        overlay_visible: false, capture_metrics: true, instant_replay: false,
        ..Default::default()
    };
    service.update_global_game_profile(global).unwrap();
    let game = service.add_game(AddGameRequest::new("Overlay fixture", "/bin/sleep")).unwrap();
    let other = service.add_game(AddGameRequest::new("Other fixture", "/bin/true")).unwrap();
    let library = f.root.join("fixture-layer.so");
    std::fs::write(&library, b"fixture only; never loaded").unwrap();
    let capture = service.start_capture_session(&CaptureSessionConfig::new(f.root.join("runtime"), library)).unwrap();
    let mut plan = f.plan("/bin/sleep", &["30"]);
    plan.capture = Some(capture);
    plan.profile = game.profile.resolve(global);
    plan.request.game_id = Some(game.id);
    plan.request.metrics = true.into();
    f.engine.start(plan).unwrap();
    assert!(!f.engine.active.as_ref().unwrap().profile.overlay_visible);
    global.overlay_visible = true;
    service.update_global_game_profile(global).unwrap();
    assert_eq!(f.engine.update_active_overlay_config(&global).unwrap(), Some(true));
    let custom = PerGameProfile { overlay_visible: Inheritable::Custom(false), ..Default::default() };
    service.update_game_profile(game.id, custom).unwrap();
    assert_eq!(f.engine.update_game_overlay_config(other.id).unwrap(), None);
    assert!(f.engine.active.as_ref().unwrap().profile.overlay_visible);
    assert_eq!(f.engine.update_game_overlay_config(game.id).unwrap(), Some(false));
    assert_eq!(f.engine.update_active_overlay_config(&global).unwrap(), Some(false), "custom off wins over global on");
    global.overlay_visible = false;
    service.update_global_game_profile(global).unwrap();
    service.update_game_profile(game.id, PerGameProfile { overlay_visible: Inheritable::Custom(true), ..Default::default() }).unwrap();
    assert_eq!(f.engine.update_game_overlay_config(game.id).unwrap(), Some(true));
    assert_eq!(f.engine.update_active_overlay_config(&global).unwrap(), Some(true), "custom on wins over global off");
    service.update_game_profile(game.id, PerGameProfile::default()).unwrap();
    assert_eq!(f.engine.update_game_overlay_config(game.id).unwrap(), Some(false), "reset to global applies live");
    let active = f.engine.active.as_ref().unwrap();
    assert!(active.profile.capture_metrics);
    assert!(!active.profile.instant_replay);
    f.engine.finish().unwrap();
    assert_eq!(f.engine.update_game_overlay_config(game.id).unwrap(), None);
}

#[test]
fn overlay_profile_without_capture_is_saved_for_the_next_launch() {
    let mut f = Fixture::new();
    let game = f.engine.service.add_game(AddGameRequest::new("No capture", "/bin/sleep")).unwrap();
    let mut plan = f.plan("/bin/sleep", &["30"]);
    plan.request.game_id = Some(game.id);
    f.engine.start(plan).unwrap();
    f.engine.service.update_game_profile(game.id, PerGameProfile {
        overlay_visible: redunar_core::Inheritable::Custom(true), ..Default::default()
    }).unwrap();
    assert_eq!(f.engine.update_game_overlay_config(game.id).unwrap(), None);
    assert!(!f.engine.active.as_ref().unwrap().profile.overlay_visible);
    assert!(f.engine.service.load_game_catalog().unwrap().games.iter().find(|saved| saved.id==game.id).unwrap().profile.resolve(GlobalGameProfile::default()).overlay_visible);
}

#[test]
fn hidden_runtime_can_show_hide_and_show_without_enabling_other_features() {
    let mut f = Fixture::new();
    let service = f.engine.service.clone();
    let mut global = GlobalGameProfile {
        capture_metrics: false, overlay_visible: false, instant_replay: false,
        ..Default::default()
    };
    service.update_global_game_profile(global).unwrap();
    let game = service.add_game(AddGameRequest::new("Hidden fixture", "/bin/sleep")).unwrap();
    let library = f.root.join("hidden-fixture-layer.so");
    std::fs::write(&library, b"fixture only; never loaded").unwrap();
    let capture = service.start_capture_session(&CaptureSessionConfig::new(f.root.join("runtime"), library)).unwrap();
    let mut plan = f.plan("/bin/sleep", &["30"]);
    plan.capture = Some(capture);
    plan.profile = game.profile.resolve(global);
    plan.request.game_id = Some(game.id);
    f.engine.start(plan).unwrap();
    for visible in [true, false, true, false] {
        global.overlay_visible = visible;
        service.update_global_game_profile(global).unwrap();
        assert_eq!(f.engine.update_active_overlay_config(&global).unwrap(), Some(visible));
        let active = f.engine.active.as_ref().unwrap();
        assert!(active.capture, "visibility cannot detach the runtime");
        assert!(!active.profile.capture_metrics && !active.profile.instant_replay);
    }
    f.engine.finish().unwrap();
    assert_eq!(f.engine.update_active_overlay_config(&global).unwrap(), None);
}
