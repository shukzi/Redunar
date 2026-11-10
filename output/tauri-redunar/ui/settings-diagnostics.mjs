import {escapeHtml} from './workspace-state.mjs';

const readinessRows=[
 ['encoded_packet_ring','Encoded packet ring'],
 ['atomic_clip_store','Atomic clip store'],
 ['live_frame_transfer','Live frame transfer'],
 ['hardware_encoder','Hardware encoder'],
 ['playable_container','Playable container'],
 ['performance_budget','Performance budget'],
];

export function diagnosticsSnapshotView(snapshot) {
 if(!snapshot)return {rows:'',details:''};
 const readiness=snapshot.replay_readiness||{};
 const readyCount=readinessRows.filter(([key])=>readiness[key]).length;
 const rows=[
  ['Monitor samples',snapshot.hardware_samples],
  ['Game scans',snapshot.game_scans],
  ['Replay readiness',`${readyCount}/6 ready`],
  ['Capture runtime',snapshot.capture_runtime],
  ['Shortcut monitor',snapshot.shortcut_state||'Unknown'],
  ['Replay frames received',snapshot.received_frame_count],
  ['Encoded packets',snapshot.encoded_packet_count],
  ['Audio packets',snapshot.audio_packet_count],
  ['Audio bytes',snapshot.audio_byte_count],
 ].map(([label,value])=>`<div data-diagnostics-snapshot-row><span>${label}</span><b>${escapeHtml(value??'—')}</b></div>`).join('');
 const lastUpdate=snapshot.captured_at_unix_ms==null?'—':new Date(snapshot.captured_at_unix_ms).toLocaleTimeString([], {hour:'2-digit',minute:'2-digit',second:'2-digit'});
 const duration=ms=>ms==null?'—':`${ms} ms`;
 const overruns=snapshot.hardware_overruns+snapshot.game_scan_overruns;
 const shortcuts=snapshot.shortcut_message||'Redunar handles its shortcuts while it runs.';
 const gpuAccess=snapshot.encoder_candidates?.length
  ? snapshot.encoder_candidates.map(device=>`${device.driver} (${device.accessible?'access available':'access unavailable'})`).join(', ')
  : 'No GPU devices found for the Replay check.';
 const issue=snapshot.telemetry_error||snapshot.game_detection_error;
 const details=`<div class="diagnostics-readiness">
  <div class="diagnostics-subheading"><h3>Replay readiness checks</h3><span>Backend verification</span></div>
  ${readinessRows.map(([key,label])=>`<div class="diagnostics-readiness-row"><span>${label}</span><span class="pill ${readiness[key]?'green':'red'}">${readiness[key]?'Ready':'Not verified'}</span></div>`).join('')}
 </div>
 <p class="small-note">Last monitor update: ${escapeHtml(lastUpdate)} · System sampling: ${escapeHtml(duration(snapshot.last_hardware_sample_ms))} · Game scan: ${escapeHtml(duration(snapshot.last_game_scan_ms))} · Samples or scans over their time limit: ${Number.isFinite(overruns)?overruns:'—'}</p>
 <p class="small-note">Keyboard shortcuts: ${escapeHtml(shortcuts)}</p>
 <p class="small-note">Replay GPU device access: ${escapeHtml(gpuAccess)}</p>
 ${issue?`<p class="diagnostics-error">${escapeHtml(issue)}</p>`:''}`;
 return {rows,details};
}
