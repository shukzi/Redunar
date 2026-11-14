// Synthetic native responses for isolated production-UI tests.
const fixtureResolutionRejection="This game resolution cannot be recorded at the selected frame rate. Lower the game's resolution or recording frame rate.";

window.bridge={calls:[],errors:[],unexpectedCommands:[],conflict:false,unavailable:false,automaticUpdates:true,startupUpdateAvailable:true,revision:1,modules:{frame_metrics:'Enabled',in_game_overlay:'Enabled',instant_replay:'Enabled'}};
window.addEventListener('error',e=>bridge.errors.push(e.message));
window.addEventListener('unhandledrejection',e=>bridge.errors.push(String(e.reason)));
function artworkFixture(width,height,label,mime='image/png'){const canvas=document.createElement('canvas');canvas.width=width;canvas.height=height;const ctx=canvas.getContext('2d');const gradient=ctx.createLinearGradient(0,0,width,height);gradient.addColorStop(0,'#8b202a');gradient.addColorStop(1,'#171717');ctx.fillStyle=gradient;ctx.fillRect(0,0,width,height);ctx.fillStyle='#ffffff';ctx.font='32px sans-serif';ctx.fillText(label,30,70);return Array.from(atob(canvas.toDataURL(mime).split(',')[1]),c=>c.charCodeAt(0));}
const posterFixture=artworkFixture(300,450,'Poster');
const bannerFixture=artworkFixture(1200,350,'Landscape banner');
const captureFixture=artworkFixture(640,360,'Capture fixture','image/jpeg');
const values={overlay:true,preset:'Compact',layout:'Telemetry',palette:'Redunar',branding:true,position:'Top left',scale:100,opacity:90,metrics:['FPS'],captureMetrics:true,replay:true,fps:60,quality:'Balanced',format:'MKV',shortcuts:{overlay:'Ctrl+Shift+R',30:'Ctrl+Shift+S'}};
if(new URLSearchParams(location.search).get('overlay')==='fps-only'){
 Object.assign(values,{preset:'FPS only',layout:'Ribbon',metrics:['GPU temperature']});
}
function replayPreferencesFixture(){return {microphone_mode:bridge.microphone||'off',microphone_source:bridge.source||null,microphone_gain_db:bridge.microphoneGain??0,initial_save_duration_seconds:30,resolved_directory:(bridge.folder??'/fixture/Videos')+'/Redunar Replays',custom_save_parent:bridge.folder??null,close_overlay_on_outside_click:bridge.outsideClick!==false};}
const games=[{id:'1',name:'Test game with a long local catalog title',executable:'/usr/bin/steam',arguments:['--one','--two'],working_directory:null,steam_app_id:42,revision:'1',launch_revision:'launch-1',overrides:{}},{id:'2',name:'Second local game',executable:'/fixture/second',arguments:[],revision:'1',launch_revision:'launch-2',overrides:{overlay:false}}];
const catalogCase=new URLSearchParams(location.search).get('catalog');
if(catalogCase){bridge.catalog=catalogCase==='populated'?games:[];bridge.idle=true;bridge.startupUpdateAvailable=false;bridge.catalogPending=catalogCase==='pending';bridge.catalogFailure=catalogCase==='failed';}
const nativeHandlers={
'save_replay':async(args)=>{
 if(bridge.failSave)throw Error('Fixture save failed');
 return {};
},
'replay_runtime_status':async(args)=>{
 if(bridge.failRuntime)throw Error('Fixture runtime unavailable');
 if(bridge.replayRuntime)return structuredClone(bridge.replayRuntime);
 if(bridge.replayUnavailable)return {phase:'Unavailable',can_save:false,buffered_seconds:0,unavailable_reason:fixtureResolutionRejection};
 if(bridge.replayFailure)return {phase:'Failed',can_save:false,buffered_seconds:0,failure:'The video encoder failed. Check Replay hardware support, then relaunch the game.'};
 if(bridge.replayWaiting)return {phase:'Inactive',can_save:false,buffered_seconds:0,pending_reason:'Waiting for recordable game frames. Game startup or a display change can pause Replay.'};
 if(bridge.replayInactive)return {phase:'Inactive',can_save:false,buffered_seconds:0};
 return {phase:'Buffering',can_save:true,buffered_seconds:30,received_frame_count:600,encoded_packet_count:600,audio_packet_count:1500,audio_byte_count:4500,audio_active:true,completed_save_revision:1};
},
'replay_clips':async(args)=>{
 if(bridge.failClips)throw Error('Fixture inventory unavailable');
 if(bridge.holdClips){const snapshot=structuredClone(bridge.clips);await new Promise(resolve=>bridge.releaseClips=resolve);return snapshot;}
 return bridge.clips??=Array.from({length:3},(_,i)=>({file_name:`redunar-replay-1789254367-629872093-${i+1}-a-long-capture-filename.mkv`,title:`Capture ${i+1}`,game_name:i===0?games[0].name:null,bytes:68000000,modified_unix_ns:'1789300000000000000',duration_seconds:30}));
},
'daemon_status':async(args)=>{
 if(bridge.failDaemon)throw Error('Fixture native status unavailable');
 return bridge.unavailable?{available:false}:{available:true,diagnostic_log_status:bridge.logStatus||'off',revision:++bridge.revision,cpu_model:'Fixture CPU',cpu_utilization_percent:32,cpu_temperature_celsius:61,gpu_model:'Fixture GPU',gpu_utilization_percent:81,gpu_temperature_celsius:67,ram_used_bytes:12884901888,ram_total_bytes:34359738368,vram_used_bytes:5368709120,vram_total_bytes:12884901888};
},
'session_status':async(args)=>{
 if(bridge.idle)return {phase:'Idle',can_end:false,launch_locked:false,history_revision:1};
 return {phase:'Running',game:games[0].name,can_end:true,launch_locked:true,elapsed_seconds:1938,captures_saved:3,profile_label:'Session profile',feature_summary:'Metrics enabled · replay enabled',restoration:'NotRequired',history_revision:1,measurements:{revision:bridge.revision,average_fps:141,frame_time_ms:7.1,one_percent_low_fps:118,point_one_percent_low_fps:97,frame_intervals_ns:[7100000+Math.round(Math.sin(bridge.revision)*700000)],phase:'Live'}};
},
'module_statuses':async(args)=>{
 return structuredClone(bridge.modules);
},
'discover_games':async(args)=>{
 return structuredClone(bridge.discovered||[]);
},
'import_discovered_games':async(args)=>{
 if(true){await new Promise(resolve=>setTimeout(resolve,180));if(bridge.failImport)throw Error('Another Redunar app owns the backend session. Close it, then reopen this app before editing saved data.');return structuredClone(games);}
},
'catalog_games':async(args)=>{
 if(true){if(bridge.catalogPending)await new Promise(resolve=>bridge.releaseCatalog=resolve);if(bridge.catalogFailure)throw Error('Fixture catalog unavailable');return structuredClone(bridge.catalog??games);}
},
'game_installation_statuses':async(args)=>{
 if(true){if(bridge.installationFailure)throw Error('Fixture library unavailable');return bridge.installation||{'1':'installed','2':'installed'};}
},
'game_poster':async(args)=>{
 return posterFixture;
},
'game_banner':async(args)=>{
 return args.gameId==='1'?bannerFixture:null;
},
'clip_metadata':async(args)=>{
 return {duration_seconds:30,width:640,height:360,fps:60};
},
'clip_thumbnail':async(args)=>{
 return bridge.brokenThumbnail?[0,1,2]:captureFixture;
},
'replay_storage_status':async(args)=>{
 return {used_bytes:204000000,available_bytes:bridge.lowDisk?950000000:137400000000,total_bytes:512000000000};
},
'microphone_levels':async(args)=>{
 return null;
},
'set_microphone_gain':async(args)=>{
 if(true){bridge.microphoneGain=args.gainDb;return replayPreferencesFixture();}
},
'start_microphone_test':async(args)=>{
 if(true)throw Error('Microphone test requires the dedicated audio fixture');
},
'stop_microphone_test':async(args)=>{
 return null;
},
'microphone_inputs':async(args)=>{
 if(true){if(bridge.failInputs)throw Error('Fixture audio server unavailable');return [{id:'usb-mic',label:'USB microphone'},{id:'internal-mic',label:'Internal microphone'}];}
},
'pick_replay_folder':async(args)=>{
 return bridge.pickedFolder??null;
},
'set_replay_save_parent':async(args)=>{
 if(true){bridge.folder=args.parent;return replayPreferencesFixture();}
},
'set_clip_details':async(args)=>{
 if(true){await new Promise(r=>setTimeout(r,80));if(bridge.failFavorite)throw Error('Fixture favorite persistence failed');const c=bridge.clips.find(c=>c.file_name===args.fileName);c.favorite=args.favorite;return {file_name:c.file_name,favorite:c.favorite};}
},
'rename_replay_clip':async(args)=>{
 if(true){await new Promise(r=>setTimeout(r,80));if(bridge.failRename)throw Error('A clip with that filename already exists');const c=bridge.clips.find(c=>c.file_name===args.fileName);c.file_name=args.newFileName;return {file_name:c.file_name,favorite:Boolean(c.favorite)};}
},
'app_preferences':async(args)=>{
 return {close_to_tray:false,automatic_updates:bridge.automaticUpdates,diagnostic_log:bridge.logRequested===true,diagnostic_log_status:bridge.logStatus||'off'};
},
'set_close_to_tray':async(args)=>{
 return {close_to_tray:args.enabled,automatic_updates:bridge.automaticUpdates};
},
'set_diagnostic_log':async(args)=>{
 if(true){bridge.logRequested=args.enabled;return {diagnostic_log:args.enabled,diagnostic_log_status:bridge.logStatus||'off'};}
},
'set_automatic_updates':async(args)=>{
 if(true){bridge.automaticUpdates=args.enabled;return {close_to_tray:false,automatic_updates:bridge.automaticUpdates};}
},
'check_for_updates':async(args)=>{
 if(true){
  if(bridge.installedUpdate)return {current_version:'0.1.2',state:'restart-needed',message:'Redunar 0.1.3 installed. Fully quit Redunar, including its tray process, then reopen it.',latest_version:'0.1.3',package_kind:'arch',asset_name:'redunar-app-linux-x86_64.pkg.tar.zst'};
  if(bridge.startupUpdateAvailable){bridge.startupUpdateAvailable=false;return {current_version:'0.1.2',state:'available',message:'Verified update: Redunar 0.1.3.',latest_version:'0.1.3',package_kind:'arch',asset_name:'redunar-app-linux-x86_64.pkg.tar.zst'};}
  return {current_version:'0.1.3',state:'not-configured',message:'Signed update checks are not configured in this build.'};
 }
},
'install_update':async(args)=>{
 return {state:'handoff',message:'The installer finished. Installation is unconfirmed. Check for updates or retry.'};
},
'global_settings':async(args)=>{
 return {values:structuredClone(values),revision:'1'};
},
'save_global_settings':async(args)=>{
 if(true){if(bridge.conflict)throw new Error('Saved defaults changed elsewhere. Reload before saving.');Object.assign(values,args.input);return {values:structuredClone(values),revision:'2'};}
},
'save_game_profile':async(args)=>{
 if(true){const game=games.find(g=>g.id===args.gameId);game.overrides=Object.fromEntries(Object.entries(args.values).filter(([,v])=>v!==null));const overlay=game.overrides.overlay??values.overlay;return {games:structuredClone(games),liveNotice:!bridge.idle&&args.gameId==='1'?`Game settings saved. Current game overlay set to ${overlay?'show':'hide'}.`:'Game settings saved for the next game.'};}
},
'update_game_launch':async(args)=>{
 if(true){bridge.launchArgs=args.arguments;return structuredClone(games);}
},
'save_shortcuts':async(args)=>{
 if(true){values.shortcuts=args.shortcuts;return {values:structuredClone(values)};}
},
'set_replay_overlay_behavior':async(args)=>{
 if(true){bridge.outsideClick=args.closeOnOutsideClick;return {...replayPreferencesFixture(),overlay_shortcut:values.shortcuts.overlay||''};}
},
'steam_setup_status':async(args)=>{
 if(true){if(bridge.failSteamSetup)throw Error('Fixture Steam setup unavailable');return bridge.steamSetup||{available:true,configured:false,app_id:42,configuration_state:'not-configured',status:'Not configured',launch_options:'/usr/bin/redunar-steam-launch --app-id 42 -- %command%'};}
},
'set_replay_microphone':async(args)=>{
 if(true){await new Promise(resolve=>setTimeout(resolve,80));if(bridge.failMicrophone)throw Error('Fixture microphone write failed');bridge.microphone=args.mode;bridge.source=args.source||null;return replayPreferencesFixture();}
},
'replay_preferences':async(args)=>{
 return replayPreferencesFixture();
},
'replay_display_capability':async(args)=>{
 return {connected_outputs:0,status:'0 display output(s) discovered'};
},
'activate_shortcuts':async(args)=>{
 bridge.shortcutUnavailable=false;
 return nativeHandlers.shortcut_status(args);
},
'shortcut_status':async(args)=>{
 if(bridge.keyboardWaiting)return {state:'Unavailable',message:'No readable keyboard input device. Waiting for keyboard access to return.'};
 if(bridge.shortcutUnavailable)return {state:'Unavailable',message:'No readable keyboard input device'};
 return {state:Object.values(values.shortcuts).some(Boolean)?'Active':'Inactive'};
},
'session_history':async(args)=>{
 return bridge.historyFixture??[0,1,2].map((n)=>({id:n+1,game:n===2?'Other local game':games[0].name,started_unix:1789300000-n*86400,duration_seconds:1800,average_fps:141-n,one_percent_low_fps:118,point_one_percent_low_fps:97,frame_intervals_ns:[7100000,7300000],timeline:Array.from({length:96},(_,i)=>({elapsed_seconds:i*1800/95,fps:135+Math.sin(i)*6,frame_time_ms:7.1+Math.sin(i)*.3,cpu_temperature_celsius:61,gpu_temperature_celsius:67,cpu_utilization_percent:32,gpu_utilization_percent:81}))}));
},
'diagnostics_snapshot':async(args)=>{
 return {};
},
'clip_playback_path':async(args)=>{
 throw Error('Fixture playback preparation unavailable');
},
'cancel_clip_playback':async(args)=>{
 return null;
},
'cancel_clip_export':async(args)=>{
 return null;
},
'clip_export_status':async(args)=>{
 return {active:false,progress:0};
},
'open_replay_folder':async(args)=>{
 return null;
},
'open_clip_external':async(args)=>{
 return null;
},
'open_diagnostic_log_folder':async(args)=>{
 return null;
},
'pick_executable':async(args)=>{
 return null;
},
'launch_game':async(args)=>{
 return null;
},
'end_session':async(args)=>{
 return null;
},
'restart_redunar':async(args)=>{
 throw Error('Fixture restart unavailable');
},
'trashed_replay_clips':async(args)=>{
 return [];
},
'trash_replay_clip':async(args)=>{
 throw Error('Trash mutation requires a dedicated fixture');
},
'restore_replay_clips':async(args)=>{
 throw Error('Trash mutation requires a dedicated fixture');
},
'prepare_trash_deletion':async(args)=>{
 throw Error('Trash deletion requires a dedicated fixture');
},
'delete_trashed_replay_clips':async(args)=>{
 throw Error('Trash deletion requires a dedicated fixture');
},
'export_replay_clip':async(args)=>{
 throw Error('Export requires a dedicated fixture');
},
'plugin:window|minimize':async(args)=>{
 return null;
},
'plugin:window|toggle_maximize':async(args)=>{
 return null;
},
'plugin:window|close':async(args)=>{
 return null;
},
'plugin:window|start_resize_dragging':async(args)=>{
 return null;
},
'plugin:window|start_dragging':async(args)=>{
 return null;
},
};

const requiredArguments={
 game_banner:{gameId:'string'},game_poster:{gameId:'string'},steam_setup_status:{gameId:'string'},
 clip_metadata:{fileName:'string'},clip_thumbnail:{fileName:'string'},
 save_global_settings:{input:'object'},save_game_profile:{gameId:'string',values:'object'},
 update_game_launch:{gameId:'string'},save_shortcuts:{shortcuts:'object'},
 set_close_to_tray:{enabled:'boolean'},set_automatic_updates:{enabled:'boolean'},set_diagnostic_log:{enabled:'boolean'},
 set_microphone_gain:{gainDb:'number'},set_clip_details:{fileName:'string'},rename_replay_clip:{fileName:'string',newFileName:'string'},
};
function fixtureCommandError(command,message){
 const detail={command,message};
 bridge.unexpectedCommands.push(detail);
 window.webkit?.messageHandlers?.fixture?.postMessage(JSON.stringify(detail));
 return new Error(message);
}
bridge.setHandler=(command,handler)=>{nativeHandlers[command]=handler;};
window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'}},invoke:async(command,args={})=>{
 bridge.calls.push({command,args:structuredClone(args)});
 if(!Object.hasOwn(nativeHandlers,command))throw fixtureCommandError(command,'Unexpected native fixture command: '+command);
 if(!args||typeof args!=='object'||Array.isArray(args))throw fixtureCommandError(command,'Invalid arguments for '+command);
 for(const [key,type] of Object.entries(requiredArguments[command]||{})){
  const value=args[key];
  if(typeof value!==type||(type==='object'&&(!value||Array.isArray(value))))
   throw fixtureCommandError(command,'Invalid '+key+' for '+command);
 }
 return nativeHandlers[command](args);
}};
window.check=(condition,message)=>{if(!condition)throw new Error(message)};
window.q=s=>document.querySelector(s);window.qa=s=>Array.from(document.querySelectorAll(s));
window.click=s=>{const el=q(s);check(!!el,'Missing '+s);check(!el.disabled,'Disabled '+s);el.click();};
