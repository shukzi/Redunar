import {test} from 'node:test';
import assert from 'node:assert/strict';
import {loadClipPreview} from '../ui/clip-library.mjs';
test('thumbnail paints before selected clip packet inspection',async()=>{
 const events=[];const item={file_name:'fixture'};let release;
 const loaded=loadClipPreview({item,call:async(command,args)=>{events.push(args.includeFps?'details':command);if(args.includeFps)await new Promise(resolve=>release=resolve);return command==='clip_thumbnail'?[1]:{duration_seconds:30};},thumbnailDataUri:()=>'preview',updateMetadata:()=>events.push('metadata painted'),updateThumbnail:()=>events.push('thumbnail painted'),selected:()=>true});
 await new Promise(resolve=>setImmediate(resolve));
 assert.deepEqual(events,['clip_metadata','metadata painted','clip_thumbnail','thumbnail painted','details']);
 release();await loaded;assert.equal(item.duration,30);
});
test('offscreen or deselected cards do not request a packet scan',async()=>{
 let scans=0;
 await loadClipPreview({item:{file_name:'fixture'},call:async(command,args)=>{if(args.includeFps)scans++;return {};},thumbnailDataUri:()=>null,updateMetadata:()=>{},updateThumbnail:()=>{},selected:()=>false});
 assert.equal(scans,0);
});
