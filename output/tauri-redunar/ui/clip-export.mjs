// Unified copy form. The native host validates and owns all media work.
const escape = value => String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const clock = value => Number.isFinite(value)?`${Math.floor(value/60).toString().padStart(2,'0')}:${Math.floor(value%60).toString().padStart(2,'0')}`:'—';
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
// Capture this once when opening the dialog. Neither a later player selection nor
// Retry may replace the source/range. Exact equality avoids rounding away trims.
export function clipExportSnapshot({fileName,thumbnail='',sourceBytes=null,sourceDuration=null,start,end,selectionStart=start,selectionEnd=end,copyRange,options={}}) {
 const value=normalizeExportOptions(options);
 const known=Number.isFinite(sourceDuration)&&sourceDuration>0;
 const mode=copyRange??(value.compression||(known&&start===0&&end===sourceDuration)?'entire':'selection');
 if(!['entire','selection'].includes(mode))throw Error('Choose the entire clip or selected range.');
 return Object.freeze({fileName,thumbnail,sourceBytes,sourceDuration:known?sourceDuration:null,selectionStart,selectionEnd,copyRange:mode});
}

function exportRange(snapshot,mode) {
 if(!['entire','selection'].includes(mode))throw Error('Choose the entire clip or selected range.');
 const start=mode==='entire'?0:snapshot.selectionStart,end=mode==='entire'?snapshot.sourceDuration:snapshot.selectionEnd;
 if(!Number.isFinite(start)||!Number.isFinite(end)||start<0||end>86400||end-start<.1
  ||(snapshot.sourceDuration!==null&&end>snapshot.sourceDuration+.05))throw Error('Wait for a valid clip range of at least 0.1 seconds.');
 return {start,end};
}

function clipExportSource({fileName,thumbnail='',sourceBytes=null,start,end,copyRange='selection'}) {
 const whole=copyRange==='entire';
 return `<div class="clip-export-source"><div class="clip-export-thumbnail">${thumbnail?`<img src="${escape(thumbnail)}" alt="">`:svg('M4 5h16v14H4zM10 9l5 3-5 3z')}</div><div class="clip-export-source-info"><strong title="${escape(fileName)}">${escape(fileName)}</strong><div class="clip-export-range"><span data-export-range-label>${whole?'Entire clip':'Selected range'}</span><span aria-hidden="true">·</span><strong data-export-range-time>${whole?clock(end):`${clock(start)}–${clock(end)}`}</strong></div>${clipExportSize(sourceBytes)?`<div class="clip-export-original">Original · ${clipExportSize(sourceBytes)}</div>`:''}</div></div>`;
}

export function clipExportProgressView({fileName,thumbnail='',sourceBytes=null,start,end,copyRange,options={},progress=0}) {
 const value=normalizeExportOptions(options);
 return `<section class="clip-export-running" aria-label="Export progress">
 ${clipExportSource({fileName,thumbnail,sourceBytes,start,end,copyRange:copyRange??(value.compression?'entire':'selection')})}
 <div class="clip-export-progress"><div><span>Exporting</span><output id="clip-export-percent">${Math.round(progress)}%</output></div><progress id="clip-export-progress" max="100" value="${progress}"></progress></div>
 <p id="clip-export-output" class="clip-export-output">Preparing clip</p>
 <div class="dialog-actions"><button class="button" type="button" data-export-cancel>Cancel export</button><button class="button primary" type="button" data-export-working disabled aria-busy="true">${clipExportSpinner()}Exporting…</button></div></section>`;
}

export function clipExportForm(context) {
 const {fileName,thumbnail='',sourceBytes=null,outputName,options={},retry=false}=context;
 const snapshot=clipExportSnapshot(context),{sourceDuration,selectionStart,selectionEnd,copyRange}=snapshot;
 const value=normalizeExportOptions(options),limited=value.compression||value.maxSizeMb!==null,whole=copyRange==='entire';
 const start=whole?0:selectionStart,end=whole?sourceDuration:selectionEnd;
 let validSelection=false;try{exportRange(snapshot,'selection');validSelection=true;}catch{}
 const hasSelection=validSelection&&(sourceDuration===null||selectionStart!==0||selectionEnd!==sourceDuration);
 const rangeChoices=[...(sourceDuration!==null||whole?[['entire','Entire clip']]:[]),...(hasSelection||!whole?[['selection','Selected range']]:[])];
 const size=compressionSizePlan({sourceBytes,sourceDuration,start,end,compression:whole,maxSizeMb:value.maxSizeMb}),limit=size.limitMb,maximum=size.available?size.maxMb:.1;
 const baseline=size.baselineBytes===null?'Size unavailable':`${whole?'Original ·':'Selection ≈'} ${clipExportSize(size.baselineBytes)}`;
 return `<form id="trim-export-form" class="compact-clip-form clip-export-options" data-size-limited="${limited}" data-size-available="${size.available}" data-baseline-bytes="${size.baselineBytes??''}">
 <input type="hidden" name="fileName" value="${escape(fileName)}"><input type="hidden" name="start" value="${escape(start??'')}"><input type="hidden" name="end" value="${escape(end??'')}"><input type="hidden" name="compression" value="${limited&&whole}"><input type="hidden" name="sourceBytes" value="${clipExportSize(sourceBytes)?sourceBytes:''}"><input type="hidden" name="sourceDuration" value="${sourceDuration??''}"><input type="hidden" name="selectionStart" value="${escape(selectionStart??'')}"><input type="hidden" name="selectionEnd" value="${escape(selectionEnd??'')}"><input type="hidden" name="thumbnail" value="${escape(thumbnail)}">
 ${clipExportSource({fileName,thumbnail,sourceBytes,start,end,copyRange})}
 <div class="clip-copy-range"><label for="export-range">Range</label>${rangeChoices.length>1?`<select id="export-range" name="copyRange" aria-label="Export range">${choices(rangeChoices,copyRange)}</select>`:`<input type="hidden" name="copyRange" value="${copyRange}"><span>${whole?'Entire clip':'Selected range'}</span>`}</div>
 <div class="clip-export-format"><label for="export-format">Format</label><select id="export-format" name="outputFormat" aria-label="Export format">${choices([['mp4','MP4'],['mkv','MKV']],value.outputFormat)}</select></div>
 <div class="form-label"><label for="export-filename">Filename</label><div class="clip-export-filename"><input id="export-filename" name="outputName" required maxlength="200" value="${escape(outputName)}"><span data-export-extension>.${value.outputFormat}</span></div></div>
 <section class="clip-compression-controls clip-export-foldout" aria-label="Compression options">
 <div class="clip-copy-compression-row"><label for="export-compress">Compress copy</label><label class="switch"><input id="export-compress" name="limitSize" type="checkbox" aria-label="Compress copy" aria-controls="export-size-controls" aria-expanded="${limited}" ${limited?'checked':''}><span></span></label></div>
 <div id="export-size-controls" data-export-size-controls><div class="clip-export-size-inner"><div class="clip-export-size-heading"><label for="export-size">Maximum size</label></div><div class="clip-export-size-value" data-export-size-field><input id="export-size" name="maxSizeMb" type="number" min="0.1" max="${maximum}" step="0.1" value="${limit}" aria-label="Maximum file size in MB" aria-describedby="export-size-note export-size-status"><span>MB</span></div>
 <input type="range" min="0.1" max="${maximum}" step="0.1" value="${limit}" data-export-size-slider aria-label="Maximum file size slider"><div class="clip-export-size-scale"><span>0.1 MB</span><span data-export-maximum>${size.available?`${maximum} MB`:'—'}</span></div>
 <div class="clip-export-size-comparison"><span data-export-baseline>${baseline}</span>${svg('M5 12h14m-5-5 5 5-5 5')}<strong>Under <output data-export-limit-label>${limit} MB</output></strong></div>
 <p id="export-size-status" class="clip-export-size-status" role="status"></p></div></div>
 <p class="clip-export-sr-only" id="export-size-note">Maximum size for the separate copy. Smaller files can lose detail, especially during movement.</p></section>
 <p data-export-range-status class="clip-export-size-status" role="status"></p>
 <p class="clip-copy-original-note">Your original recording stays unchanged.</p>
 <div class="dialog-actions"><button class="button" type="button" data-close>Cancel</button><button class="button primary" type="submit">${retry?'Retry export':'Export'}</button></div></form>`;
}

const formSnapshots=new WeakMap();
const numericField=field=>field.value===''?null:Number(field.value);
function formSnapshot(form) {
 if(!formSnapshots.has(form)) {
  const f=form.elements;
  formSnapshots.set(form,clipExportSnapshot({fileName:f.fileName.value,thumbnail:f.thumbnail.value,sourceBytes:numericField(f.sourceBytes),sourceDuration:numericField(f.sourceDuration),selectionStart:numericField(f.selectionStart),selectionEnd:numericField(f.selectionEnd),copyRange:f.copyRange.value}));
 }
 return formSnapshots.get(form);
}

export function bindClipExportForm(form,paintSlider=()=>{}) {
 if(!form)return;
 const snapshot=formSnapshot(form),fields=form.elements,slider=form.querySelector('[data-export-size-slider]');
 let previousRange=fields.copyRange.value;
 function sync() {
  const mode=fields.copyRange.value,whole=mode==='entire',limited=fields.limitSize.checked;
  let range,error='';try{range=exportRange(snapshot,mode);}catch(failure){error=failure.message;}
  const {start,end}=range??{start:null,end:null};
  fields.start.value=start??'';fields.end.value=end??'';fields.compression.value=String(limited&&whole);
  const current=numericField(fields.maxSizeMb);
  const plan=compressionSizePlan({...snapshot,start,end,compression:whole,maxSizeMb:current});
  const available=Boolean(range)&&plan.available,baseline=plan.baselineBytes;
  form.dataset.sizeAvailable=String(available);form.dataset.baselineBytes=baseline??'';
  fields.maxSizeMb.max=slider.max=String(available?plan.maxMb:.1);
  if(previousRange!==mode)fields.maxSizeMb.value=String(plan.limitMb);
  previousRange=mode;
  // Unavailable compression can always be switched off. Never silently drop a
  // requested cap just because the context or a disabled control is missing.
  fields.limitSize.disabled=!available&&!limited;
  fields.limitSize.setAttribute('aria-expanded',String(limited));
  fields.maxSizeMb.disabled=!limited||!available;slider.disabled=!limited||!available;
  fields.maxSizeMb.required=limited&&available;form.dataset.sizeLimited=String(limited);
  const controls=form.querySelector('[data-export-size-controls]');controls.inert=!limited;controls.setAttribute('aria-hidden',String(!limited));
  fields.maxSizeMb.style.setProperty('--size-digits',Math.max(2,fields.maxSizeMb.value.length));
  slider.value=fields.maxSizeMb.value;slider.setAttribute('aria-valuetext',`${fields.maxSizeMb.value} MB`);paintSlider(slider);
  const mb=numericField(fields.maxSizeMb),fits=available&&mb!==null&&mb*1000000>=baseline;
  const invalid=mb===null||!Number.isFinite(mb)||mb<.1||mb>Number(slider.max);
  form.querySelector('[data-export-maximum]').textContent=available?`${plan.maxMb} MB`:'—';
  form.querySelector('[data-export-baseline]').textContent=baseline===null?'Size unavailable':`${whole?'Original ·':'Selection ≈'} ${clipExportSize(baseline)}`;
  form.querySelector('[data-export-limit-label]').textContent=available&&mb!==null?`${fields.maxSizeMb.value} MB`:'—';
  form.querySelector('#export-size-status').textContent=!available?(baseline===null?'Size unavailable. Export without compression.':'This range is already below 0.1 MB.'):fits?'Already under this limit. Choose a smaller size.':'';
  form.querySelector('[data-export-range-status]').textContent=error;
  form.querySelector('[type=submit]').disabled=Boolean(error)||(limited&&(!available||fits||invalid));
  form.querySelector('[data-export-extension]').textContent='.'+fields.outputFormat.value;
  form.querySelector('[data-export-range-label]').textContent=whole?'Entire clip':'Selected range';
  form.querySelector('[data-export-range-time]').textContent=range?(whole?clock(end):`${clock(start)}–${clock(end)}`):'—';
 }
 slider.addEventListener('input',()=>{fields.maxSizeMb.value=slider.value;});
 fields.maxSizeMb.addEventListener('change',()=>{if(fields.maxSizeMb.value!==''&&Number.isFinite(Number(fields.maxSizeMb.value)))fields.maxSizeMb.value=String(Math.max(.1,Math.min(Number(slider.max),Number(fields.maxSizeMb.value))));});
 form.addEventListener('input',sync);form.addEventListener('change',sync);sync();
}

// This is also the complete Retry context: pass it back to clipExportForm with
// retry:true. Invoke with {fileName, startSeconds:start, endSeconds:end,
// outputName, options}; the remaining fields are UI context only.
export function readClipExportRequest(form) {
 const snapshot=formSnapshot(form),fields=form.elements,copyRange=fields.copyRange.value;
 const {start,end}=exportRange(snapshot,copyRange),whole=copyRange==='entire',limited=fields.limitSize.checked;
 const maxSizeMb=limited?numericField(fields.maxSizeMb):null;
 if(limited) {
  const plan=compressionSizePlan({...snapshot,start,end,compression:whole,maxSizeMb});
  if(!plan.available)throw Error('Size unavailable for compression. Export without compression.');
  if(maxSizeMb===null||!Number.isFinite(maxSizeMb)||maxSizeMb<.1||maxSizeMb>plan.maxMb)throw Error(`Choose a maximum size between 0.1 and ${plan.maxMb} MB.`);
  if(maxSizeMb*1000000>=plan.baselineBytes)throw Error('Already under this limit. Choose a smaller size.');
 }
 // Native compression means inspected WHOLE source, not "enable a size cap".
 // Uncapped whole copies use the validated duration; selected caps must stay false.
 const options=Object.freeze(normalizeExportOptions({outputFormat:fields.outputFormat.value,maxSizeMb,compression:whole&&limited}));
 const outputName=fields.outputName.value.trim();
 if(!outputName||new TextEncoder().encode(outputName).length>200)throw Error('Use an export filename of 1–200 bytes.');
 return Object.freeze({...snapshot,copyRange,start,end,outputName,options});
}

export function readClipExportOptions(form) {
 return readClipExportRequest(form).options;
}
