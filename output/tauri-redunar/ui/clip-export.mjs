// Shared Size first form. The native host validates and owns all media work.
const escape = value => String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const clock = value => `${Math.floor(value/60).toString().padStart(2,'0')}:${Math.floor(value%60).toString().padStart(2,'0')}`;
const choices = (entries,value) => entries.map(([key,label])=>`<option value="${key}" ${String(value)===key?'selected':''}>${label}</option>`).join('');

export const clipExportSize = bytes => typeof bytes==='number'&&Number.isFinite(bytes)&&bytes>=0?`${(bytes/1000000).toFixed(2)} MB`:null;
export function clipExportProgressLabel(status={}) {
 const phase={preparing:'Preparing clip',analysing:'Analysing clip',encoding:'Exporting clip',checking:'Checking clip',saving:'Saving clip'}[status.phase]||'Preparing clip';
 const size=['encoding','checking','saving'].includes(status.phase)?clipExportSize(status.output_bytes):null;
 return size?`${phase} · ${size}`:phase;
}

export function normalizeExportOptions({outputFormat='mp4',maxSizeMb=null,compression=false}={}) {
 if(!['mp4','mkv'].includes(outputFormat))throw Error('Choose MP4 or MKV.');
 if(maxSizeMb!==null&&(!Number.isFinite(Number(maxSizeMb))||Number(maxSizeMb)<.1||Number(maxSizeMb)>500))throw Error('Choose a maximum size between 0.1 and 500 MB.');
 return {outputFormat,maxSizeMb:maxSizeMb===null?null:Number(maxSizeMb),compression:Boolean(compression)};
}

// Range bytes are a duration-based estimate, never a measured trimmed file.
export function compressionSizePlan({sourceBytes=null,sourceDuration=null,start=0,end=0,compression=false,maxSizeMb=null}={}) {
 const measured=typeof sourceBytes==='number'&&Number.isFinite(sourceBytes)&&sourceBytes>0;
 const range=Number.isFinite(sourceDuration)&&sourceDuration>0&&Number.isFinite(start)&&Number.isFinite(end)&&start>=0&&end>start&&end<=sourceDuration+.05;
 const baselineBytes=measured&&(compression||range)?sourceBytes*(compression?1:Math.min(1,(end-start)/sourceDuration)):null;
 // Round down to the control's tenth-MB step, so its maximum cannot exceed
 // either the source/selection baseline or the encoder's existing ceiling.
 const maxMb=baselineBytes===null?null:Math.min(500,Math.floor(baselineBytes/100000)/10);
 const available=maxMb!==null&&maxMb>=.1;
 const preferred=maxSizeMb??(baselineBytes===null?.1:Math.floor(baselineBytes/200000)/10);
 return {baselineBytes,estimated:!compression,maxMb,available,limitMb:available?Math.max(.1,Math.min(maxMb,preferred)):.1};
}

const svg=path=>`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
export const clipExportSpinner = () => '<span class="clip-export-spinner" aria-hidden="true"></span>';
function clipExportSource({fileName,thumbnail='',sourceBytes=null,start,end,compression=false}) {
 return `<div class="clip-export-source"><div class="clip-export-thumbnail">${thumbnail?`<img src="${escape(thumbnail)}" alt="">`:svg('M4 5h16v14H4zM10 9l5 3-5 3z')}</div><div class="clip-export-source-info"><strong title="${escape(fileName)}">${escape(fileName)}</strong><div class="clip-export-range"><span>${compression?'Entire clip':'Selected range'}</span><span aria-hidden="true">·</span><strong>${compression?clock(end):`${clock(start)}–${clock(end)}`}</strong></div>${clipExportSize(sourceBytes)?`<div class="clip-export-original">Original · ${clipExportSize(sourceBytes)}</div>`:''}</div></div>`;
}

export function clipExportProgressView({fileName,thumbnail='',sourceBytes=null,start,end,options={},progress=0}) {
 const value=normalizeExportOptions(options);
 return `<section class="clip-export-running" aria-label="Export progress">
 ${clipExportSource({fileName,thumbnail,sourceBytes,start,end,compression:value.compression})}
 <div class="clip-export-progress"><div><span>Exporting clip</span><output id="clip-export-percent">${Math.round(progress)}%</output></div><progress id="clip-export-progress" max="100" value="${progress}"></progress></div>
 <p id="clip-export-output" class="clip-export-output">Preparing clip</p>
 <div class="dialog-actions"><button class="button" type="button" data-export-cancel>Cancel export</button><button class="button primary" type="button" data-export-working disabled aria-busy="true">${clipExportSpinner()}Exporting…</button></div></section>`;
}

export function clipExportForm({fileName,thumbnail='',sourceBytes=null,sourceDuration=null,start,end,outputName,options={},retry=false}) {
 const value=normalizeExportOptions(options),limited=value.maxSizeMb!==null;
 const size=compressionSizePlan({sourceBytes,sourceDuration,start,end,...value}),limit=size.limitMb,maximum=size.available?size.maxMb:.1;
 const baseline=size.baselineBytes===null?'Size unavailable':`${size.estimated?'Selection ≈':'Original ·'} ${clipExportSize(size.baselineBytes)}`;
 const sizeControls=`<section class="clip-compression-controls ${value.compression?'':'clip-export-foldout'}" aria-label="Compression options">
 ${value.compression?'':`<input type="checkbox" name="limitSize" hidden ${limited?'checked':''}><button type="button" class="clip-export-compress-toggle" data-export-compress aria-expanded="${limited}" aria-controls="export-size-controls"><span>Compress</span>${svg('M6 9l6 6 6-6')}</button>`}
 <div id="export-size-controls" data-export-size-controls><div class="clip-export-size-inner"><div class="clip-export-size-heading"><label for="export-size">Maximum size</label></div><div class="clip-export-size-value" data-export-size-field><input id="export-size" name="maxSizeMb" type="number" min="0.1" max="${maximum}" step="0.1" value="${limit}" aria-label="Maximum file size in MB" aria-describedby="export-size-note export-size-status"><span>MB</span></div>
 <input type="range" min="0.1" max="${maximum}" step="0.1" value="${limit}" data-export-size-slider aria-label="Maximum file size slider"><div class="clip-export-size-scale"><span>0.1 MB</span><span>${size.available?`${maximum} MB`:'—'}</span></div>
 <div class="clip-export-size-comparison"><span>${baseline}</span>${svg('M5 12h14m-5-5 5 5-5 5')}<strong>Under <output data-export-limit-label>${limit} MB</output></strong></div>
 <p id="export-size-status" class="clip-export-size-status" role="status"></p></div></div>
 <p class="clip-export-sr-only" id="export-size-note">Maximum size for the separate copy. Smaller files can lose detail, especially during movement.</p></section>`;
 return `<form id="trim-export-form" class="compact-clip-form clip-export-options" data-size-limited="${value.compression||limited}" data-size-available="${size.available}" data-baseline-bytes="${size.baselineBytes??''}">
 <input type="hidden" name="fileName" value="${escape(fileName)}"><input type="hidden" name="start" value="${start}"><input type="hidden" name="end" value="${end}"><input type="hidden" name="compression" value="${value.compression}"><input type="hidden" name="sourceBytes" value="${clipExportSize(sourceBytes)?sourceBytes:''}"><input type="hidden" name="sourceDuration" value="${Number.isFinite(sourceDuration)?sourceDuration:''}">
 ${clipExportSource({fileName,thumbnail,sourceBytes,start,end,compression:value.compression})}
 ${value.compression?sizeControls:''}
 <div class="clip-export-format"><label for="export-format">Format</label><select id="export-format" name="outputFormat" aria-label="Export format">${choices([['mp4','MP4'],['mkv','MKV']],value.outputFormat)}</select></div>
 <div class="form-label"><label for="export-filename">Filename</label><div class="clip-export-filename"><input id="export-filename" name="outputName" required maxlength="200" value="${escape(outputName)}"><span data-export-extension>.${value.outputFormat}</span></div></div>
 ${value.compression?'':sizeControls}
 <div class="dialog-actions"><button class="button" type="button" data-close>Cancel</button><button class="button primary" type="submit">${retry?'Retry':value.compression?'Compress clip':'Export'}</button></div></form>`;
}

export function bindClipExportForm(form,paintSlider) {
 if(!form)return;
 const fields=form.elements,slider=form.querySelector('[data-export-size-slider]');
 function sync() {
  const limited=fields.compression.value==='true'||fields.limitSize.checked;
  const available=form.dataset.sizeAvailable==='true',baseline=Number(form.dataset.baselineBytes),mb=Number(fields.maxSizeMb.value);
  fields.maxSizeMb.disabled=!limited||!available;slider.disabled=!limited||!available;
  fields.maxSizeMb.required=limited&&available;
  form.dataset.sizeLimited=String(limited);
  form.querySelector('[data-export-compress]')?.setAttribute('aria-expanded',String(limited));
  const controls=form.querySelector('[data-export-size-controls]');
  controls.inert=!limited;controls.setAttribute('aria-hidden',String(!limited));
  fields.maxSizeMb.style.setProperty('--size-digits',Math.max(2,fields.maxSizeMb.value.length));
  slider.value=fields.maxSizeMb.value;
  slider.setAttribute('aria-valuetext',`${fields.maxSizeMb.value} MB`);
  paintSlider(slider);
  const fits=available&&fields.maxSizeMb.value!==''&&mb*1000000>=baseline;
  form.querySelector('[data-export-limit-label]').textContent=available&&fields.maxSizeMb.value!==''?`${fields.maxSizeMb.value} MB`:'—';
  form.querySelector('#export-size-status').textContent=!available?(form.dataset.baselineBytes===''?'Size unavailable. Export without compression.':fields.compression.value==='true'?'This clip is already below 0.1 MB.':'This selection is estimated below 0.1 MB.'):fits?(fields.compression.value==='true'?'Already under this limit. Choose a smaller size.':'Already under this limit, based on the selection estimate.'):'';
  form.querySelector('[type=submit]').disabled=fields.compression.value==='true'&&(!available||fits);
  form.querySelector('[data-export-extension]').textContent='.'+fields.outputFormat.value;
 }
 slider.addEventListener('input',()=>{fields.maxSizeMb.value=slider.value;});
 fields.maxSizeMb.addEventListener('change',()=>{if(fields.maxSizeMb.value!==''&&Number.isFinite(Number(fields.maxSizeMb.value)))fields.maxSizeMb.value=String(Math.max(.1,Math.min(Number(slider.max),Number(fields.maxSizeMb.value))));});
 form.querySelector('[data-export-compress]')?.addEventListener('click',()=>{fields.limitSize.checked=!fields.limitSize.checked;sync();});
 form.addEventListener('input',sync);form.addEventListener('change',sync);sync();
}

export function readClipExportOptions(form) {
 const fields=form.elements,compression=fields.compression.value==='true';
 // The simplified dialog keeps source resolution and frame rate automatically.
 return normalizeExportOptions({outputFormat:fields.outputFormat.value,maxSizeMb:(compression||fields.limitSize.checked)&&!fields.maxSizeMb.disabled?Number(fields.maxSizeMb.value):null,compression});
}
