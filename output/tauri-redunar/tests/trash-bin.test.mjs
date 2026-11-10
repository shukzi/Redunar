import test from 'node:test';
import assert from 'node:assert/strict';
import {trashBinMarkup,trashDeletionMarkup,trashDeletionResultMarkup} from '../ui/trash-bin.mjs';
test('bin exposes explicit deletion next to restore and only offers clear when populated',()=>{
 const html=trashBinMarkup([{file_name:'<clip>.mkv',bytes:1000000}]);
 assert.match(html,/Restore<\/button>.*Delete permanently/);
 assert.match(html,/Clear bin/);assert.match(html,/&lt;clip&gt;.mkv/);
 assert.doesNotMatch(trashBinMarkup([]),/clear-trash-bin|delete-trash-clip/);
});
test('confirmation lists exact native targets and carries only its token',()=>{
 const html=trashDeletionMarkup({token:'12',entries:[{file_name:'one.mkv',bytes:2000000},{file_name:'two.mkv',bytes:1000000}]});
 assert.match(html,/2 clips \(3.0 MB\)/);assert.match(html,/one.mkv/);assert.match(html,/two.mkv/);
 assert.match(html,/data-token="12"/);assert.match(html,/cannot be undone/);
});
test('partial failures remain explicit and safely escaped',()=>{
 const html=trashDeletionResultMarkup({deleted:['one.mkv'],failures:[['<other>.mkv','Changed & restored']]});
 assert.match(html,/1 clip permanently deleted/);assert.match(html,/&lt;other&gt;.mkv/);assert.match(html,/Changed &amp; restored/);
});
