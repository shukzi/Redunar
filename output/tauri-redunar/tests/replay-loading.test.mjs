import {test} from 'node:test';
import assert from 'node:assert/strict';
import {watchReplayLoading} from '../ui/replay-loading.mjs';
const wait=()=>new Promise(resolve=>setTimeout(resolve,40));

test('metadata alone does not leave a stalled player loading forever',async()=>{
 const video=new EventTarget(),controller=new AbortController();let expired=0;
 watchReplayLoading(video,controller.signal,()=>expired++,15);
 video.dispatchEvent(new Event('loadedmetadata'));
 await wait();assert.equal(expired,1);
 await wait();assert.equal(expired,1);
});

test('a decoded first frame cancels the loading deadline',async()=>{
 const video=new EventTarget(),controller=new AbortController();let expired=false;
 watchReplayLoading(video,controller.signal,()=>{expired=true;},15);
 video.dispatchEvent(new Event('loadeddata'));
 await wait();assert.equal(expired,false);
});

test('switching clips or a media error cancels the previous deadline',async()=>{
 for(const event of ['abort','error']){
  const video=new EventTarget(),controller=new AbortController();let expired=false;
  watchReplayLoading(video,controller.signal,()=>{expired=true;},15);
  if(event==='abort')controller.abort();else video.dispatchEvent(new Event('error'));
  await wait();assert.equal(expired,false);
 }
});
