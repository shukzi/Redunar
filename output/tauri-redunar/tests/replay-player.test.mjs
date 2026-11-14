import test from 'node:test';
import assert from 'node:assert/strict';
import {replayPlayer} from '../ui/replay-player.mjs';

const tick=()=>new Promise(resolve=>setImmediate(resolve));
class Video extends EventTarget {
 dataset={};paused=true;duration=10;currentTime=0;
 pause(){this.paused=true;this.dispatchEvent(new Event('pause'));}
 removeAttribute(name){delete this[name];}
 load(){}
}
function fixture() {
 let video=new Video(),selected={id:'a',file_name:'a.mkv'};
 const requests=[],calls=[],nodes=new Map();
 for(const id of ['media-state-overlay','media-state-title','media-state-detail','retry-clip','clip-preview-status'])nodes.set('#'+id,{});
 const player=replayPlayer({query:selector=>selector==='#clip-video'?video:nodes.get(selector)||null,
  selected:()=>selected,call:(command,args)=>{
   calls.push({command,args});
   return command==='clip_playback_path'?new Promise((resolve,reject)=>requests.push({resolve,reject})):Promise.resolve();
  },native:true,applyVolume(){},updateMetadata(){},updateSlider(){},setControlHint(){},icon(){},spinner(){},
  time:String,preciseTime:String,exporting:()=>false,notify(){}});
 return {player,requests,calls,nodes,get video(){return video;},
  replace(){video=new Video();selected={id:'b',file_name:'b.mkv'};}};
}

test('Selection changes cancel preparation and keep a stale URL off the new player',async()=>{
 const f=fixture(),old=f.video;
 f.player.load();f.player.reset();f.replace();f.player.load();
 assert.equal(f.calls.filter(c=>c.command==='cancel_clip_playback').length,1);
 f.requests[0].resolve('obsolete');await tick();
 assert.equal(old.src,undefined);assert.equal(f.video.src,undefined);
 f.requests[1].resolve('current');await tick();
 assert.equal(f.video.src,'current');
 f.player.reset();
 assert.equal(f.video.src,undefined);
 assert.deepEqual(f.player.range(),{ready:false,duration:0,start:0,end:0});
});

test('A failed current video detaches media callbacks and preserves retry feedback',async()=>{
 const f=fixture();
 f.player.load();f.requests[0].resolve('fixture-url');await tick();
 f.video.dispatchEvent(new Event('error'));
 assert.equal(f.nodes.get('#media-state-title').textContent,'Clip could not load');
 assert.match(f.nodes.get('#media-state-detail').textContent,/Retry or open/);
 assert.equal(f.nodes.get('#retry-clip').hidden,false);
 // A late callback from the failed media must not restore its duration/readiness.
 f.video.dispatchEvent(new Event('loadeddata'));
 assert.deepEqual(f.player.range(),{ready:false,duration:0,start:0,end:0});
 f.player.reset();
});
