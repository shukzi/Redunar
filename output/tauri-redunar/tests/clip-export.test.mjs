import {test} from 'node:test';
import assert from 'node:assert/strict';
import {normalizeExportOptions,clipExportForm,readClipExportOptions,clipExportSize,clipExportProgressLabel,clipExportProgressView,compressionSizePlan,bindClipExportForm,readClipExportRequest,clipExportSnapshot} from '../ui/clip-export.mjs';

test('compression uses the source size, with a smaller default and no larger allowance',()=>{
 const whole=compressionSizePlan({sourceBytes:80000000,compression:true});
 assert.equal(whole.maxMb,80);assert.equal(whole.limitMb,40);assert.equal(whole.estimated,false);
 assert.equal(compressionSizePlan({sourceBytes:80000000,compression:true,maxSizeMb:100}).limitMb,80);
 assert.equal(compressionSizePlan({sourceBytes:1234567,compression:true}).maxMb,1.2);
 assert.equal(compressionSizePlan({sourceBytes:1500000000,compression:true}).maxMb,500);
});
test('selection estimates use full duration, including retries; tiny and missing sources have no invented range',()=>{
 const selected=compressionSizePlan({sourceBytes:80000000,sourceDuration:120,start:30,end:60});
 assert.equal(selected.baselineBytes,20000000);assert.equal(selected.maxMb,20);assert.equal(selected.limitMb,10);assert.equal(selected.estimated,true);
 for(const args of [{sourceBytes:80000000},{sourceBytes:null,compression:true},{sourceBytes:90000,compression:true}])assert.equal(compressionSizePlan(args).available,false);
 const form=clipExportForm({fileName:'lap.mp4',sourceBytes:80000000,sourceDuration:120,start:30,end:60,outputName:'lap',options:{maxSizeMb:9.9},retry:true});
 assert.match(form,/Selection ≈ 20.00 MB/);assert.match(form,/max="20"/);assert.match(form,/name="sourceDuration" value="120"/);assert.match(form,/data-export-limit-label>9.9 MB/);
});

test('export accepts both containers and decimal MB limits, rejects invalid budgets',()=>{
 assert.deepEqual(normalizeExportOptions({outputFormat:'mkv',maxSizeMb:'9.9',compression:true}),{outputFormat:'mkv',maxSizeMb:9.9,compression:true});
 for(const value of [0,-1,'',NaN,Infinity,501])assert.throws(()=>normalizeExportOptions({maxSizeMb:value}),/maximum size/);
 assert.throws(()=>normalizeExportOptions({outputFormat:'avi'}),/MP4 or MKV/);
});
test('original and output sizes use measured decimal bytes, with no invented analysis size',()=>{
 assert.equal(clipExportSize(1234567),'1.23 MB');
 for(const value of [null,undefined,NaN,-1,'123'])assert.equal(clipExportSize(value),null);
 assert.equal(clipExportProgressLabel({phase:'encoding',output_bytes:1234567}),'Exporting clip · 1.23 MB');
 assert.equal(clipExportProgressLabel({phase:'encoding',output_bytes:0}),'Exporting clip · 0.00 MB');
 assert.equal(clipExportProgressLabel({phase:'analysing',output_bytes:1234567}),'Analysing clip');
 const args={fileName:'lap.mp4',start:1,end:2,outputName:'lap',sourceBytes:1234567};
 assert.match(clipExportForm(args),/Original · 1.23 MB/);
 assert.match(clipExportForm({...args,retry:true}),/name="sourceBytes" value="1234567"/);
 assert.ok(!clipExportForm({...args,sourceBytes:null}).includes('Original ·'));
});
test('export progress retains escaped source context and offers cancellation without another submission',()=>{
 const view=clipExportProgressView({fileName:'lap <one>.mp4',sourceBytes:2500000,start:2,end:8});
 assert.match(view,/lap &lt;one&gt;\.mp4/);
 assert.match(view,/00:02–00:08/);
 assert.match(view,/Original · 2.50 MB/);
 assert.match(view,/data-export-working disabled aria-busy="true"/);
 assert.match(view,/data-export-cancel/);
 assert.match(view,/clip-export-spinner/);
 assert.ok(!view.includes('type="submit"'));
});

// Small DOM adapter for the actual rendered form/binder. No media/native bridge:
// assertions exercise the arguments that production will pass to Tauri.
const decode=value=>value.replace(/&quot;/g,'"').replace(/&#39;/g,"'").replace(/&lt;/g,'<').replace(/&gt;/g,'>').replace(/&amp;/g,'&');
function mountedForm(context) {
 const markup=clipExportForm(context),nodes=new Map(),elements={};
 const attrs=tag=>Object.fromEntries([...tag.matchAll(/([\w-]+)="([^"]*)"/g)].map(([,key,value])=>[key,decode(value)]));
 const form={elements,dataset:new Proxy({},{set(target,key,value){target[key]=String(value);return true;}}),listeners:{},addEventListener(type,fn){(this.listeners[type]??=[]).push(fn);},querySelector(selector){if(!nodes.has(selector))nodes.set(selector,node({}));return nodes.get(selector);}};
 function node(attributes) {
  let fieldValue=String(attributes.value??'');
  const result={checked:/checked/.test(attributes.raw??''),disabled:false,required:false,style:{setProperty(){}},listeners:{},...attributes,
   setAttribute(key,value){this[key]=String(value);},addEventListener(type,fn){(this.listeners[type]??=[]).push(fn);},
   fire(type){for(const fn of this.listeners[type]??[])fn();for(const fn of form.listeners[type]??[])fn();}};
  Object.defineProperty(result,'value',{get:()=>fieldValue,set:value=>{fieldValue=String(value);}});
  return result;
 }
 for(const [tag] of markup.matchAll(/<input\b[^>]*>/g)) {
  const attributes=attrs(tag),input=node({...attributes,raw:tag});
  if(attributes.name)elements[attributes.name]=input;
  if(tag.includes('data-export-size-slider'))nodes.set('[data-export-size-slider]',input);
 }
 for(const [,attributes,options] of markup.matchAll(/<select\b([^>]*)>([\s\S]*?)<\/select>/g)) {
  const entries=[...options.matchAll(/<option\b([^>]*)>/g)].map(([,tag])=>({...attrs(tag),selected:/selected/.test(tag)}));
  elements[attrs(attributes).name]=node({value:(entries.find(entry=>entry.selected)??entries[0]).value});
 }
 bindClipExportForm(form);
 return form;
}
const context={fileName:'source.mkv',thumbnail:'data:image/jpeg;base64,abc',sourceBytes:80000000,sourceDuration:120,start:30,end:60,outputName:'lap',options:{outputFormat:'mkv'}};
const change=(form,name,value)=>{const field=form.elements[name];if(typeof value==='boolean')field.checked=value;else field.value=String(value);field.fire('change');};

test('unified form escapes filenames and separates range from compression controls',()=>{
 const html=clipExportForm({...context,outputName:'lap <one> "small"'});
 assert.match(html,/lap &lt;one&gt; &quot;small&quot;/);
 assert.match(html,/data-export-extension>.mkv/);
 assert.match(html,/name="copyRange"/);assert.match(html,/Compress copy/);assert.match(html,/>Export</);assert.match(html,/<label for="export-range">Range<\/label>/);
 assert.ok(!html.includes('data-export-compress'));
 const form=mountedForm(context);
 assert.equal(form.elements.copyRange.value,'selection');
 assert.deepEqual(readClipExportOptions(form),{outputFormat:'mkv',maxSizeMb:null,compression:false});
});

test('selected compression preserves its range; whole compression requests native inspection',()=>{
 const form=mountedForm(context);
 change(form,'limitSize',true);
 let request=readClipExportRequest(form);
 assert.deepEqual([request.start,request.end],[30,60]);
 assert.deepEqual(request.options,{outputFormat:'mkv',maxSizeMb:10,compression:false});
 assert.equal(form.elements.compression.value,'false');
 change(form,'copyRange','entire');
 request=readClipExportRequest(form);
 assert.deepEqual([request.start,request.end],[0,120]);
 assert.equal(request.options.compression,true);
 assert.equal(form.elements.compression.value,'true');
 assert.equal(form.dataset.baselineBytes,'80000000');
 assert.equal(form.elements.maxSizeMb.max,'80');
 change(form,'limitSize',false);
 assert.deepEqual(readClipExportOptions(form),{outputFormat:'mkv',maxSizeMb:null,compression:false});
 assert.deepEqual([form.elements.start.value,form.elements.end.value],['0','120']);
 change(form,'copyRange','selection');
 assert.deepEqual([readClipExportRequest(form).start,readClipExportRequest(form).end],[30,60]);
});

test('toggling compression off excludes a retained limit, then restores it when enabled',()=>{
 const form=mountedForm(context);
 change(form,'limitSize',true);change(form,'maxSizeMb',9.9);
 change(form,'limitSize',false);
 assert.equal(form.elements.maxSizeMb.disabled,true);
 assert.equal(form.elements.maxSizeMb.value,'9.9');
 assert.equal(readClipExportOptions(form).maxSizeMb,null);
 change(form,'limitSize',true);
 assert.equal(readClipExportOptions(form).maxSizeMb,9.9);
});

test('near-whole selections are never rounded into the entire source',()=>{
 const form=mountedForm({...context,start:.02,end:119.98});
 const request=readClipExportRequest(form);
 assert.equal(request.copyRange,'selection');
 assert.deepEqual([request.start,request.end],[.02,119.98]);
 change(form,'limitSize',true);
 assert.equal(readClipExportOptions(form).compression,false);
});

test('explicit whole mode may retain a full-duration selected-range snapshot',()=>{
 const form=mountedForm({...context,start:0,end:120,copyRange:'selection'});
 assert.equal(readClipExportRequest(form).copyRange,'selection');
 change(form,'limitSize',true);
 assert.equal(readClipExportOptions(form).compression,false);
});

test('whole-entry and Retry retain the original editor snapshot, not later player globals',()=>{
 const initial=mountedForm({...context,start:0,end:120,selectionStart:30,selectionEnd:60,options:{compression:true}});
 change(initial,'outputName','retry name');change(initial,'outputFormat','mkv');change(initial,'maxSizeMb',9.9);
 const request=readClipExportRequest(initial);
 assert.equal(Object.isFrozen(request),true);assert.equal(Object.isFrozen(request.options),true);
 const retry=mountedForm({...request,retry:true});
 assert.deepEqual(readClipExportRequest(retry),request);
 assert.match(clipExportForm({...request,retry:true}),/Retry export/);
 change(retry,'copyRange','selection');
 assert.deepEqual([readClipExportRequest(retry).start,readClipExportRequest(retry).end],[30,60]);
 assert.equal(readClipExportRequest(retry).thumbnail,context.thumbnail);
 assert.equal(readClipExportOptions(retry).compression,false);
});

test('the captured source and selection cannot be replaced through mutable hidden fields',()=>{
 const form=mountedForm(context);
 form.elements.fileName.value='other.mp4';form.elements.sourceDuration.value='999';
 form.elements.selectionStart.value='0';form.elements.selectionEnd.value='999';
 form.elements.start.value='0';form.elements.end.value='999';form.elements.compression.value='true';
 const request=readClipExportRequest(form);
 assert.equal(request.fileName,context.fileName);
 assert.deepEqual([request.start,request.end],[30,60]);
 assert.equal(request.options.compression,false);
});

test('missing/tiny context allows uncapped valid selections, never silently removes requested compression',()=>{
 for(const patch of [{sourceBytes:null},{sourceBytes:90000},{sourceDuration:null}]) {
  const form=mountedForm({...context,...patch});
  assert.equal(form.querySelector('[type=submit]').disabled,false);
  assert.equal(readClipExportOptions(form).maxSizeMb,null);
  change(form,'limitSize',true);
  assert.equal(form.querySelector('[type=submit]').disabled,true);
  assert.throws(()=>readClipExportOptions(form),/Size unavailable/);
  // Disabled size input must not turn the requested limited export into uncapped output.
  form.elements.maxSizeMb.disabled=true;
  assert.throws(()=>readClipExportOptions(form),/Size unavailable/);
  change(form,'limitSize',false);
  assert.equal(form.querySelector('[type=submit]').disabled,false);
 }
});

test('unknown duration cannot submit an entire copy, including legacy compression entry',()=>{
 const form=mountedForm({...context,sourceDuration:null,start:0,end:120,options:{compression:true}});
 assert.equal(form.querySelector('[type=submit]').disabled,true);
 assert.throws(()=>readClipExportRequest(form),/valid clip range/);
});

test('invalid, short and stale ranges are rejected before native submission',()=>{
 for(const range of [{start:-1,end:2},{start:2,end:2.05},{start:0,end:Infinity},{start:0,end:121},{start:0,end:86401}]) {
  const form=mountedForm({...context,...range,copyRange:'selection'});
  assert.equal(form.querySelector('[type=submit]').disabled,true);
  assert.throws(()=>readClipExportRequest(form),/valid clip range/);
 }
 const form=mountedForm(context);form.elements.copyRange.value='tampered';
 assert.throws(()=>readClipExportRequest(form),/entire clip or selected range/);
 assert.throws(()=>clipExportSnapshot({...context,copyRange:'tampered'}),/entire clip or selected range/);
});

test('field/slider clamp on commit, and already-fitting limits cannot submit',()=>{
 const form=mountedForm(context);change(form,'limitSize',true);
 const slider=form.querySelector('[data-export-size-slider]');
 slider.value='7.5';slider.fire('input');
 assert.equal(readClipExportOptions(form).maxSizeMb,7.5);
 form.elements.maxSizeMb.value='999';form.elements.maxSizeMb.fire('input');
 assert.throws(()=>readClipExportOptions(form),/between 0.1 and 20/);
 form.elements.maxSizeMb.fire('change');assert.equal(form.elements.maxSizeMb.value,'20');
 assert.equal(form.querySelector('[type=submit]').disabled,true);
 assert.throws(()=>readClipExportOptions(form),/Already under/);
 for(const bad of ['', 'NaN','-1']) {
  form.elements.maxSizeMb.value=bad;form.elements.maxSizeMb.fire('input');
  assert.equal(form.querySelector('[type=submit]').disabled,true);
  assert.throws(()=>readClipExportOptions(form),/maximum size/);
 }
});

test('native option shape stays exact and invalid filename/container are rejected',()=>{
 const form=mountedForm(context);
 assert.deepEqual(Object.keys(readClipExportOptions(form)).sort(),['compression','maxSizeMb','outputFormat']);
 change(form,'outputFormat','avi');assert.throws(()=>readClipExportRequest(form),/MP4 or MKV/);
 change(form,'outputFormat','mp4');change(form,'outputName','');assert.throws(()=>readClipExportRequest(form),/filename/);
 change(form,'outputName','é'.repeat(101));assert.throws(()=>readClipExportRequest(form),/filename/);
});

test('uncapped whole-copy progress uses explicit mode without inventing compression',()=>{
 const view=clipExportProgressView({...context,start:0,end:120,copyRange:'entire',options:{compression:false}});
 assert.match(view,/Entire clip/);assert.match(view,/<span>Exporting<\/span>/);assert.match(view,/data-export-cancel/);
});


test('ordinary full-range copy needs no compression flag or limit',()=>{
 const form=mountedForm({...context,start:0,end:120});
 const request=readClipExportRequest(form);
 assert.equal(request.copyRange,'entire');
 assert.deepEqual([request.start,request.end],[0,120]);
 assert.deepEqual(request.options,{outputFormat:'mkv',maxSizeMb:null,compression:false});
});

test('a selected limited request round-trips through Retry with identical native arguments',()=>{
 const form=mountedForm(context);
 change(form,'limitSize',true);change(form,'maxSizeMb',8.8);change(form,'outputFormat','mp4');
 const request=readClipExportRequest(form);
 const retry=mountedForm({...request,retry:true});
 assert.deepEqual(readClipExportRequest(retry),request);
 assert.equal(retry.querySelector('[data-export-size-controls]').inert,false);
 change(retry,'limitSize',false);
 assert.equal(retry.querySelector('[data-export-size-controls]').inert,true);
 assert.equal(readClipExportOptions(retry).compression,false);
 assert.equal(readClipExportOptions(retry).maxSizeMb,null);
});
