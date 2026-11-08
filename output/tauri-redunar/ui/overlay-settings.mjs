// The editor changes presentation only. The existing global draft and delegated
// native commands remain the owners of every setting and Save/Discard action.
import {refreshWorkspacePresentation} from './workspace-presentation.mjs';
const escape=value=>String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
let editorTab='appearance';
let previewScene=null;
const previewObserver=new ResizeObserver(fitOverlayPreview);
function fitOverlayPreview() {
 const hud=previewScene?.querySelector('#hud');
 if(!hud||!previewScene.clientWidth||!previewScene.clientHeight)return;
 const scale=Number(hud.style.getPropertyValue('--hud-scale'))||1;
 const fit=Math.max(.01,Math.min(1,(previewScene.clientWidth-32)/(hud.offsetWidth*scale),(previewScene.clientHeight-32)/(hud.offsetHeight*scale)));
 const value=String(Number(fit.toFixed(5)));
 if(hud.style.getPropertyValue('--hud-preview-fit')!==value)hud.style.setProperty('--hud-preview-fit',value);
}
const presets=['FPS only','Compact','Detailed','Custom'];
const tabAttributes=key=>`role="tab" id="overlay-editor-tab-${key}" data-overlay-editor-tab="${key}" aria-controls="overlay-editor-panel-${key}" aria-selected="${editorTab===key}" tabindex="${editorTab===key?0:-1}" class="${editorTab===key?'active':''}"`;
const panelAttributes=key=>`role="tabpanel" id="overlay-editor-panel-${key}" aria-labelledby="overlay-editor-tab-${key}" data-overlay-editor-panel="${key}" aria-hidden="${editorTab!==key}" ${editorTab===key?'':'data-editor-inactive inert'}`;
export const overlayVisibilityDescription=()=>'Save changes to apply overlay visibility. Per-game overrides still apply.';

export function overlaySettings({preview,draft,shown,metricNames,swatches,visibility,recording,appearance,live}) {
 const fpsOnly=draft.preset==='FPS only';
 const layout=`<div class="overlay-setting-row overlay-layout-block"><div class="overlay-setting-copy"><h3>Layout</h3><span class="overlay-layout-hint" id="overlay-layout-hint" hidden>Layout applies to Compact, Detailed and Custom presets.</span></div><div class="layout-grid layout-switcher choice-switcher" data-choice-group="Overlay layout" role="group" aria-label="Overlay layout" aria-describedby="overlay-layout-hint">${['Grid','Ribbon','Telemetry'].map(name=>`<button type="button" data-layout="${name}" aria-pressed="${draft.layout===name}" ${fpsOnly?'disabled':''}><strong>${name}</strong></button>`).join('')}</div></div>`;
 const palette=`<div class="overlay-setting-row overlay-palette-block"><div class="overlay-setting-copy"><h3>Color palette</h3><span id="overlay-palette-name">${escape(draft.palette||'Redunar')}</span></div><div class="palette-grid palette-switcher choice-switcher" data-choice-group="Overlay palette" role="group" aria-label="Overlay palette">${swatches}</div></div>`;
 const groups=[['Frame metrics',metricNames.filter(name=>!/^(CPU|GPU)/.test(name))],['Hardware',metricNames.filter(name=>/^(CPU|GPU)/.test(name))]].map(([name,metrics])=>`<section class="metric-group" aria-label="${name}"><div class="metric-group-header"><h3>${name}</h3></div><div class="metric-toggles">${metrics.map(metric=>`<label class="metric-toggle-row"><span class="metric-toggle-name">${escape(metric)}</span><span class="switch"><input type="checkbox" data-metric="${escape(metric)}" aria-label="${escape(metric)}" ${shown.includes(metric)?'checked':''} ${draft.preset==='Custom'?'':'disabled'}><span></span></span></label>`).join('')}</div></section>`).join('');
 const liveCard=`<section class="live-overlay-panel"><div class="overlay-field-row"><div><div class="overlay-setting-label"><h3>Show overlay</h3></div><p class="overlay-visibility-description">${overlayVisibilityDescription(live)}</p></div>${visibility}</div></section>`;
 const sessionCard=`<section class="session-metrics-panel"><div class="overlay-field-row"><div><h3>Record session metrics</h3><p>Records frame timing in future sessions.</p></div>${recording}</div></section>`;
 return `<div class="global-grid overlay-settings-page"><section class="panel controls-panel overlay-settings-panel"><section class="overlay-runtime-section"><div class="section-heading"><h2>Session behavior</h2></div><div class="overlay-preview-features">${liveCard}${sessionCard}</div></section><section class="overlay-preview-panel" aria-label="Overlay preview">${preview}</section></section>
 <section class="panel controls-panel overlay-settings-editor"><div class="tabbar overlay-editor-tabs" role="tablist" aria-label="Overlay editor"><button type="button" ${tabAttributes('appearance')}>Appearance</button><button type="button" ${tabAttributes('metrics')}>Metrics</button></div><div class="overlay-editor-panels">
 <section class="panel overlay-style overlay-editor-panel" ${panelAttributes('appearance')}>${palette}${layout}<section class="overlay-appearance-controls" aria-label="Overlay appearance"><div class="overlay-appearance-fields">${appearance}</div></section></section>
 <section class="panel overlay-information overlay-editor-panel" ${panelAttributes('metrics')}><div class="overlay-setting-row overlay-preset-row"><div class="overlay-setting-copy"><h3>Information preset</h3><span class="overlay-metric-count">${shown.length} metrics shown</span></div><div class="preset-switcher choice-switcher" data-choice-group="Information preset" role="group" aria-label="Information preset">${presets.map(name=>`<button type="button" data-preset="${name}" aria-pressed="${draft.preset===name}" tabindex="${draft.preset===name?0:-1}">${name}</button>`).join('')}</div></div><div class="overlay-metric-header"><h2 class="metrics-title">Displayed metrics</h2></div><p class="metric-hint">Use Custom to choose individual metrics.</p><p class="metric-validation" id="metric-validation" role="status"></p><div class="metric-groups">${groups}</div></section>
 </div></section></div>`;
}

// Session and draft updates keep the explanation stable. Visibility changes
// require Save both while idle and during a game.
export function updateOverlaySessionState(live,root=document) {
 const panel=root.querySelector('.live-overlay-panel');if(!panel)return;
 const description=panel.querySelector('.overlay-visibility-description'),text=overlayVisibilityDescription(live);
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
 const choice=event.target.closest('.layout-switcher > button,.palette-switcher > button,.preset-switcher > button');
 if(choice&&['ArrowLeft','ArrowRight','Home','End'].includes(event.key)){
  const choices=[...choice.parentElement.querySelectorAll('button:not(:disabled)')];
  if(!choices.length)return;
  event.preventDefault();
  const index=event.key==='Home'?0:event.key==='End'?choices.length-1:(choices.indexOf(choice)+(event.key==='ArrowRight'?1:-1)+choices.length)%choices.length;
  choices[index].focus({preventScroll:true});choices[index].click();return;
 }
 const tab=event.target.closest('[data-overlay-editor-tab]');
 if(!tab||!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;
 event.preventDefault();event.stopPropagation();
 const key=event.key==='Home'?'appearance':event.key==='End'?'metrics':tab.dataset.overlayEditorTab==='appearance'?'metrics':'appearance';
 selectTab(tab.closest('.overlay-settings-editor'),key,true);
});

export function updateOverlayEditor(root=document) {
 const scene=root.querySelector('.overlay-settings-panel .overlay-scene');
 if(scene!==previewScene){previewObserver.disconnect();previewScene=scene;if(scene)previewObserver.observe(scene);}
 fitOverlayPreview();
 const summary=root.querySelector('.overlay-metric-count');
 if(summary){const n=root.querySelectorAll('input[data-metric]:checked').length,text=`${n} metric${n===1?'':'s'} shown`;if(summary.textContent!==text)summary.textContent=text;}
}
