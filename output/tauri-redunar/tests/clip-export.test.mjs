import {test} from 'node:test';
import assert from 'node:assert/strict';
import {normalizeExportOptions,clipExportForm,readClipExportOptions,clipExportSize,clipExportProgressLabel,clipExportProgressView,compressionSizePlan} from '../ui/clip-export.mjs';

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
test('whole-clip and selection forms keep filenames escaped and compression scoped',()=>{
 const form=clipExportForm({fileName:'source.mkv',start:0,end:12,outputName:'lap <one> "small"',options:{outputFormat:'mkv',maxSizeMb:25,compression:true}});
 assert.ok(form.includes('Entire clip'));
 assert.ok(form.includes('lap &lt;one&gt; &quot;small&quot;'));
 assert.ok(form.includes('data-export-extension>.mkv'));
 assert.ok(!form.includes('data-export-compress'));
 const selection=clipExportForm({fileName:'source.mp4',start:2,end:8,outputName:'short'});
 assert.ok(selection.includes('Selected range'));
 assert.ok(selection.includes('data-export-compress aria-expanded="false"'));
});
test('collapsed compression never submits a stale size limit',()=>{
 const elements={compression:{value:'true'},outputFormat:{value:'mkv'},maxSizeMb:{value:'67.7'}};
 assert.deepEqual(readClipExportOptions({elements}),{outputFormat:'mkv',maxSizeMb:67.7,compression:true});
 elements.compression.value='false';elements.limitSize={checked:false};
 assert.equal(readClipExportOptions({elements}).maxSizeMb,null);
 elements.limitSize.checked=true;
 assert.equal(readClipExportOptions({elements}).maxSizeMb,67.7);
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
