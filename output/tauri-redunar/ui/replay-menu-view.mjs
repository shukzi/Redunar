import { measurement } from './native-data.mjs';
const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));

export function replayStatusCopy(value) {
 const phase=String(value?.phase||'Unavailable');
 if(value?.failure)return {title:'Replay unavailable',detail:value.failure,statusClass:'error'};
 if(value?.can_save)return {title:'Ready to save',detail:`${measurement(value.buffered_seconds)}s buffered`,statusClass:'ready'};
 if(phase==='Buffering')return {title:'Buffering replay',detail:`${measurement(value?.buffered_seconds)}s buffered · building the local replay buffer`,statusClass:'pending'};
 if(phase==='Saving')return {title:'Saving replay',detail:'Writing the selected moment to local storage…',statusClass:'pending'};
 if(phase==='Inactive')return {title:'Replay inactive',detail:'Launch a supported game through Redunar to build a local buffer.',statusClass:'pending'};
 if(phase==='Unavailable')return {title:'Replay unavailable',detail:'The local replay runtime is waiting to reconnect.',statusClass:'error'};
 return {title:'Replay unavailable',detail:'The local replay runtime is not ready yet.',statusClass:'error'};
}

export function renderReplayMenu(runtime) {
 const options=[15,30,60,120];
 const status=replayStatusCopy(runtime);
 const ready=status.statusClass==='ready';
 const statusClass=status.statusClass;
 const statusTitle=status.title;
 const statusDetail=status.detail;
 return `<section class="replay-menu-panel" aria-label="Redunar Replay menu"><div class="replay-menu-head"><div><span class="replay-menu-eyebrow"><i class="replay-menu-status ${statusClass}" id="replay-menu-status-dot"></i>REDUNAR · INSTANT REPLAY</span><h1>Save replay</h1></div><button class="replay-menu-close" data-action="hide-replay-menu" aria-label="Close replay menu">×</button></div><div class="replay-menu-status-card ${statusClass}" id="replay-menu-status-card"><span class="replay-menu-status-dot"></span><div><strong id="replay-menu-status-title">${statusTitle}</strong><small id="replay-menu-status-detail">${escape(statusDetail)}</small></div></div><div class="replay-menu-durations">${options.map(seconds=>`<button class="replay-menu-duration" data-menu-duration="${seconds}" ${ready?'':'disabled'}><span>Save last</span><strong>${seconds<60?seconds+'s':seconds/60+'m'}</strong></button>`).join('')}</div><div class="replay-menu-footer"><span id="replay-menu-feedback">${ready?'Choose how much of the recent buffer to keep.':'Replay is waiting for the local capture runtime.'}</span><button class="text-button" data-action="hide-replay-menu">Return to game</button></div></section>`;
}

export function updateReplayMenu(root, runtime, busy = false) {
 const status=replayStatusCopy(runtime);
 const dot=root.querySelector('#replay-menu-status-dot');
 if(dot)dot.className=`replay-menu-status ${status.statusClass}`;
 const card=root.querySelector('#replay-menu-status-card');
 if(card)card.className=`replay-menu-status-card ${status.statusClass}`;
 const title=root.querySelector('#replay-menu-status-title');
 if(title)title.textContent=status.title;
 const detail=root.querySelector('#replay-menu-status-detail');
 if(detail)detail.textContent=status.detail;
 const ready=status.statusClass==='ready';
 const feedback=root.querySelector('#replay-menu-feedback');
 if(feedback&&feedback.dataset.manual!=='true')feedback.textContent=ready?'Choose how much of the recent buffer to keep.':runtime?.phase==='Buffering'?'Replay is buffering locally.':'Replay is waiting for the local capture runtime.';
 for(const button of root.querySelectorAll('[data-menu-duration]'))button.disabled=!ready||busy;
}
