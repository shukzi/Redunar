// The editor changes presentation only. The existing global draft and delegated
// native commands remain the owners of every setting and Save/Discard action.
import {refreshWorkspacePresentation} from './workspace-presentation.mjs';
const escape=value=>String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
let editorTab='appearance';
const descriptions={'FPS only':'Just frame rate',Compact:'Core measurements',Detailed:'Expanded measurements',Custom:'Choose your metrics'};
const tabAttributes=key=>`role="tab" id="overlay-editor-tab-${key}" data-overlay-editor-tab="${key}" aria-controls="overlay-editor-panel-${key}" aria-selected="${editorTab===key}" tabindex="${editorTab===key?0:-1}" class="${editorTab===key?'active':''}"`;
const panelAttributes=key=>`role="tabpanel" id="overlay-editor-panel-${key}" aria-labelledby="overlay-editor-tab-${key}" data-overlay-editor-panel="${key}" aria-hidden="${editorTab!==key}" ${editorTab===key?'':'data-editor-inactive inert'}`;
export const overlayVisibilityDescription=live=>live?'Saves visibility immediately and updates the running overlay when available. Per-game overrides still apply.':'Sets the default visibility for future game sessions. Per-game overrides still apply.';
const visibilityMode=live=>live?'Auto-saved':'Saved with settings';

export function overlaySettings({preview,draft,shown,metricNames,swatches,visibility,recording,appearance,live}) {
 const fpsOnly=draft.preset==='FPS only';
 const layout=`<div><div class="overlay-option-heading"><h3>Layout</h3><span class="overlay-layout-hint" id="overlay-layout-hint" ${fpsOnly?'':'hidden'}>Not used for FPS only</span></div><div class="layout-grid" role="group" aria-label="Overlay layout" aria-describedby="overlay-layout-hint">${['Grid','Ribbon','Telemetry'].map(name=>`<button type="button" data-layout="${name}" aria-pressed="${draft.layout===name}" ${fpsOnly?'disabled':''}><strong>${name}</strong><small>${name==='Grid'?'Grouped measurements':name==='Ribbon'?'Horizontal strip':'Stacked measurements'}</small></button>`).join('')}</div></div>`;
 const palette=`<div class="overlay-palette-block"><div class="overlay-option-heading"><h3>Color palette</h3><span id="overlay-palette-name">${escape(draft.palette||'Redunar')}</span></div><div class="palette-grid" role="group" aria-label="Overlay palette">${swatches}</div></div>`;
 const groups=[['Frame metrics',metricNames.filter(name=>!/^(CPU|GPU)/.test(name))],['Hardware',metricNames.filter(name=>/^(CPU|GPU)/.test(name))]].map(([name,metrics])=>`<section class="metric-group" aria-label="${name}"><div class="metric-group-header"><h3>${name}</h3><span class="metric-group-count">${metrics.filter(metric=>shown.includes(metric)).length} of ${metrics.length}</span></div><div class="metric-toggles">${metrics.map(metric=>`<label><input type="checkbox" data-metric="${escape(metric)}" ${shown.includes(metric)?'checked':''} ${draft.preset==='Custom'?'':'disabled'}><span>${escape(metric)}</span></label>`).join('')}</div></section>`).join('');
 const liveCard=`<section class="panel live-overlay-panel"><div class="overlay-settings-heading"><h2>Live overlay</h2><span class="overlay-save-mode ${live?'auto-saved-label':''}">${visibilityMode(live)}</span></div>${visibility}</section>`;
 return `<div class="global-grid"><section class="overlay-preview-panel">${preview}<div class="overlay-preview-features">${liveCard}<section class="panel session-metrics-panel"><h2>Session metrics</h2>${recording}</section></div></section>
 <section class="overlay-settings-editor"><div class="tabbar overlay-editor-tabs" role="tablist" aria-label="Overlay editor"><button type="button" ${tabAttributes('appearance')}>Appearance</button><button type="button" ${tabAttributes('metrics')}>Metrics</button></div><div class="overlay-editor-panels">
 <section class="panel overlay-style overlay-editor-panel" ${panelAttributes('appearance')}>${palette}${layout}<section class="overlay-appearance-controls controls-panel" aria-label="Overlay appearance"><h3>Overlay appearance</h3><div class="overlay-appearance-fields">${appearance}</div></section></section>
 <section class="panel overlay-information overlay-editor-panel" ${panelAttributes('metrics')}><div class="overlay-option-heading"><h3>Information preset</h3><span class="overlay-metric-count">${shown.length} metrics shown</span></div><div class="preset-grid">${Object.entries(descriptions).map(([name,description])=>`<button type="button" data-preset="${name}" aria-pressed="${draft.preset===name}"><strong>${name}</strong><small class="preset-description">${description}</small></button>`).join('')}</div><div class="overlay-metric-header"><h2 class="metrics-title">Displayed metrics</h2></div><p class="metric-hint">${draft.preset==='Custom'?'Choose the measurements to display.':'Choose Custom to change individual metrics.'}</p><p class="metric-validation" id="metric-validation" role="status"></p><div class="metric-groups">${groups}</div></section>
 </div></section></div>`;
}

// Session polling changes the save semantics, never the control's location or
// draft. Update the copy in place so an idle/live handoff keeps focus and edits.
export function updateOverlaySessionState(live,root=document) {
 const panel=root.querySelector('.live-overlay-panel');if(!panel)return;
 const mode=panel.querySelector('.overlay-save-mode'),description=panel.querySelector('.field p');
 const label=visibilityMode(live),text=overlayVisibilityDescription(live);
 if(mode.textContent!==label)mode.textContent=label;
 mode.classList.toggle('auto-saved-label',live);
 if(description&&description.textContent!==text)description.textContent=text;
}

function selectTab(editor,key,focus=false) {
 editorTab=key;
 for(const button of editor.querySelectorAll('[data-overlay-editor-tab]')){
  const selected=button.dataset.overlayEditorTab===key;
  button.classList.toggle('active',selected);button.setAttribute('aria-selected',String(selected));button.tabIndex=selected?0:-1;
  if(selected&&focus)button.focus({preventScroll:true});
 }
 for(const panel of editor.querySelectorAll('[data-overlay-editor-panel]')){
  const selected=panel.dataset.overlayEditorPanel===key;
  panel.toggleAttribute('data-editor-inactive',!selected);panel.inert=!selected;panel.setAttribute('aria-hidden',String(!selected));
 }
 refreshWorkspacePresentation();
}
document.addEventListener('click',event=>{
 const tab=event.target.closest('[data-overlay-editor-tab]');
 if(tab)selectTab(tab.closest('.overlay-settings-editor'),tab.dataset.overlayEditorTab);
});
document.addEventListener('keydown',event=>{
 const tab=event.target.closest('[data-overlay-editor-tab]');
 if(!tab||!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;
 event.preventDefault();event.stopPropagation();
 const key=event.key==='Home'?'appearance':event.key==='End'?'metrics':tab.dataset.overlayEditorTab==='appearance'?'metrics':'appearance';
 selectTab(tab.closest('.overlay-settings-editor'),key,true);
});

export function updateOverlayEditor(root=document) {
 for(const group of root.querySelectorAll('.metric-group')){
  const inputs=[...group.querySelectorAll('input[data-metric]')];
  const count=group.querySelector('.metric-group-count'),text=`${inputs.filter(input=>input.checked).length} of ${inputs.length}`;
  if(count.textContent!==text)count.textContent=text;
 }
 const summary=root.querySelector('.overlay-metric-count');
 if(summary){const n=root.querySelectorAll('input[data-metric]:checked').length,text=`${n} metric${n===1?'':'s'} shown`;if(summary.textContent!==text)summary.textContent=text;}
 const hint=root.querySelector('.overlay-layout-hint');
 if(hint)hint.hidden=root.querySelector('.preset-grid [data-preset="FPS only"]')?.getAttribute('aria-pressed')!=='true';
}
