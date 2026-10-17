import {test} from 'node:test';
import assert from 'node:assert/strict';
import {loadClipPreview,clipPreviewLoader} from '../ui/clip-library.mjs';
test('thumbnail paints before selected clip packet inspection',async()=>{
 const events=[];const item={file_name:'fixture'};let release;
 const loaded=loadClipPreview({item,call:async(command,args)=>{events.push(args.includeFps?'details':command);if(args.includeFps)await new Promise(resolve=>release=resolve);return command==='clip_thumbnail'?[1]:{duration_seconds:30};},thumbnailDataUri:()=>'preview',updateMetadata:()=>events.push('metadata painted'),updateThumbnail:()=>events.push('thumbnail painted'),selected:()=>true});
 await new Promise(resolve=>setImmediate(resolve));
 assert.deepEqual(events,['clip_thumbnail','clip_metadata','thumbnail painted','metadata painted','details']);
 release();await loaded;assert.equal(item.duration,30);
});
test('a cached thumbnail paints without waiting for slow or failed metadata',async()=>{
 const events=[];let finishMetadata;
 const item={file_name:'cached.mkv'};
 const loaded=loadClipPreview({item,call:async(command)=>{
   if(command==='clip_thumbnail')return [1];
   await new Promise(resolve=>finishMetadata=resolve);throw Error('metadata unavailable');
 },thumbnailDataUri:()=>'cached JPEG',updateMetadata:()=>events.push('metadata'),updateThumbnail:()=>events.push('thumbnail'),selected:()=>false});
 await new Promise(resolve=>setImmediate(resolve));
 assert.deepEqual(events,['thumbnail']);assert.equal(item.thumbnail,'cached JPEG');
 finishMetadata();await loaded;
});
test('offscreen or deselected cards do not request a packet scan',async()=>{
 let scans=0;
 await loadClipPreview({item:{file_name:'fixture'},call:async(command,args)=>{if(args.includeFps)scans++;return {};},thumbnailDataUri:()=>null,updateMetadata:()=>{},updateThumbnail:()=>{},selected:()=>false});
 assert.equal(scans,0);
});

const flush=()=>new Promise(resolve=>setImmediate(resolve));
function queueFixture(count=8) {
 const items=Array.from({length:count},(_,i)=>({file_name:`clip-${i}.mkv`,thumbnail:null}));
 const requests=[],painted=[],pending=[];let selected=null;
 const loader=clipPreviewLoader({
  call:(command,args)=>{requests.push({command,...args});return new Promise((resolve,reject)=>pending.push({command,...args,resolve,reject}));},
  thumbnailDataUri:()=> 'cached JPEG',updateThumbnail:item=>painted.push(item.file_name),updateMetadata:()=>{},
  current:item=>items.includes(item),selected:item=>selected===item,
 });
 const finish=(command,error=false)=>{for(const task of pending.splice(0)){
  if(task.command!==command){pending.push(task);continue;}
  if(error)task.reject(Error('Fixture failure'));
  else task.resolve(command==='clip_thumbnail'?[1]:{duration_seconds:30});
 }};
 return {items,requests,painted,pending,loader,finish,select:item=>selected=item};
}
test('all visible cache hits drain while basic metadata is stalled, with two reads per lane',async()=>{
 const f=queueFixture();f.loader.request(f.items);await flush();
 assert.equal(f.pending.filter(task=>task.command==='clip_thumbnail').length,2);
 assert.equal(f.pending.filter(task=>task.command==='clip_metadata').length,2);
 for(let i=0;i<4;i++){f.finish('clip_thumbnail');await flush();}
 assert.equal(f.painted.length,8);
 assert.equal(f.requests.filter(task=>task.command==='clip_metadata').length,2);
 f.finish('clip_metadata');f.loader.request([]);await flush();
});
test('repeated requests do not duplicate active reads; newest visible cards replace pending work',async()=>{
 const f=queueFixture();f.loader.request(f.items);await flush();
 f.loader.request(f.items);f.loader.request([f.items[7],f.items[6]]);await flush();
 assert.equal(f.requests.length,4);
 f.finish('clip_thumbnail');f.finish('clip_metadata');await flush();
 assert.deepEqual(f.requests.filter(task=>task.command==='clip_thumbnail').map(task=>task.fileName),['clip-0.mkv','clip-1.mkv','clip-7.mkv','clip-6.mkv']);
 f.loader.request([]);f.finish('clip_thumbnail');f.finish('clip_metadata');await flush();
 assert.equal(f.requests.length,8);
});
test('a pending packet scan cannot hold later cached images',async()=>{
 const f=queueFixture(6);f.select(f.items[0]);f.loader.request(f.items);await flush();
 f.finish('clip_thumbnail');f.finish('clip_metadata');await flush();
 assert.equal(f.pending.filter(task=>task.includeFps).length,1);
 f.finish('clip_thumbnail');await flush();f.finish('clip_thumbnail');await flush();
 assert.equal(f.painted.length,6);
 f.select(null);f.loader.request([]);f.finish('clip_metadata');await flush();
});
test('metadata finishing before the image defers packet inspection until the image is ready',async()=>{
 const f=queueFixture(1);f.select(f.items[0]);f.loader.request(f.items);await flush();
 f.finish('clip_metadata');await flush();assert.equal(f.requests.some(task=>task.includeFps),false);
 f.finish('clip_thumbnail');await flush();assert.equal(f.pending.filter(task=>task.includeFps).length,1);
 f.finish('clip_metadata');await flush();assert.equal(f.items[0].metadataDetailed,true);
});
test('navigation cancels queued work and preserves completed images on return',async()=>{
 const f=queueFixture(6);f.loader.request(f.items);await flush();f.loader.request([]);
 f.finish('clip_thumbnail');f.finish('clip_metadata');await flush();assert.equal(f.requests.length,4);
 f.loader.request(f.items.slice(0,2));await flush();assert.equal(f.requests.length,4);
});
test('failed reads release slots; missing metadata retries on a later request',async()=>{
 const f=queueFixture(4);f.loader.request(f.items);await flush();
 f.finish('clip_thumbnail',true);f.finish('clip_metadata',true);await flush();
 f.finish('clip_thumbnail');f.finish('clip_metadata');await flush();
 assert.equal(f.items[0].thumbnail,'');assert.equal(f.items[2].thumbnail,'cached JPEG');
 assert.equal(f.requests.length,8);
 f.loader.request(f.items);await flush();assert.equal(f.requests.length,10);
 f.finish('clip_metadata');await flush();f.loader.request([]);
});
