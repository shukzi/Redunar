import { measurement } from './native-data.mjs';
const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));

export function replayStatusCopy(value) {
 const phase=String(value?.phase||'Unavailable');
 const seconds=measurement(value?.buffered_seconds),duration=`${seconds} ${seconds==='1'?'second':'seconds'}`;
 if(value?.failure)return {title:'Replay unavailable',detail:value.failure,statusClass:'error'};
 if(value?.unavailable_reason)return {title:'Replay unavailable',detail:value.unavailable_reason,statusClass:'error'};
 if(phase==='Inactive'&&value?.pending_reason)return {title:'Waiting for game video',detail:value.pending_reason,statusClass:'pending'};
 if(value?.can_save)return {title:'Ready to save',detail:`${duration} available to save`,statusClass:'ready'};
 if(phase==='Buffering')return {title:'Preparing replay',detail:`Collecting recent gameplay. ${duration} collected.`,statusClass:'pending'};
 if(phase==='Saving')return {title:'Saving replay',detail:'Saving the selected moment to this computer…',statusClass:'pending'};
 if(phase==='Inactive')return {title:'Replay not running',detail:'Start a supported game to use Instant Replay.',statusClass:'pending'};
 if(phase==='Unavailable')return {title:'Replay unavailable',detail:value?.unavailable_reason||'Start a supported game to use Instant Replay.',statusClass:'error'};
 return {title:'Replay unavailable',detail:'Replay is not ready yet.',statusClass:'error'};
}
