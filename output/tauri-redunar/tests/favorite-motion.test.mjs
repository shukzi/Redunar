import test from 'node:test';
import assert from 'node:assert/strict';
import {animateFavorite,cancelFavoriteBurst} from '../ui/favorite-motion.mjs';

function fixture(){
  const animations=[],listeners={},preference={matches:false};
  const doc={addEventListener(){},defaultView:{matchMedia:()=>preference,addEventListener(type,fn){listeners[type]=fn;}},createElement:()=>({
    ownerDocument:doc,children:[],style:{},getBoundingClientRect:()=>({left:100,top:100,width:40}),setAttribute(){},append(node){node.parent=this;this.children.push(node);},
    remove(){if(this.parent)this.parent.children=this.parent.children.filter(node=>node!==this);},
    animate(frames,options){let finish;const finished=new Promise(resolve=>finish=resolve);const animation={finished,finish,cancel(){this.cancelled=true;finish();}};animations.push({animation,frames,options});return animation;},
  })};
  doc.body=doc.createElement();const button=doc.createElement();button.isConnected=true;
  return {button,root:doc.body,animations,preference,listeners};
}
test('Heart bursts restart without a stale completion removing the new burst',async()=>{
  const f=fixture();animateFavorite(f.button);const first=f.animations.slice();
  assert.equal(f.root.children[0].children.length,6);
  assert.ok(f.root.children[0].children.every(node=>node.innerHTML.includes('data-lucide="heart"')));
  assert.deepEqual(first.map(item=>item.options.delay),[0,50,100,150,200,250]);
  animateFavorite(f.button);const secondRoot=f.root.children[0];
  assert.ok(first.every(item=>item.animation.cancelled));
  await Promise.all(first.map(item=>item.animation.finished));await Promise.resolve();
  assert.equal(f.root.children[0],secondRoot);
  for(const item of f.animations.slice(6))item.animation.finish();
  await Promise.all(f.animations.slice(6).map(item=>item.animation.finished));await Promise.resolve();
  assert.equal(f.root.children.length,0);
});
test('Reduced motion and detached controls do not create particles',()=>{
  const f=fixture();f.preference.matches=true;animateFavorite(f.button);assert.equal(f.animations.length,0);
  f.preference.matches=false;f.button.isConnected=false;animateFavorite(f.button);assert.equal(f.animations.length,0);
});

test('Removing a favorite cancels its remaining hearts without stale completion effects',async()=>{
  const f=fixture();animateFavorite(f.button);
  const first=f.animations.slice();
  cancelFavoriteBurst(f.button);
  assert.equal(f.root.children.length,0);
  assert.ok(first.every(item=>item.animation.cancelled));
  cancelFavoriteBurst(f.button);
  animateFavorite(f.button);const next=f.root.children[0];
  await Promise.all(first.map(item=>item.animation.finished));await Promise.resolve();
  assert.equal(f.root.children[0],next);
  cancelFavoriteBurst(f.button);
});

test('A removed filtered row can finish its acknowledged burst, and navigation cancels it',()=>{
  const f=fixture();f.button.isConnected=false;
  animateFavorite(f.button,{left:100,top:80,width:30});
  assert.equal(f.root.children[0].style.left,'115px');assert.equal(f.root.children[0].style.top,'80px');
  f.listeners.hashchange();assert.equal(f.root.children.length,0);
  assert.ok(f.animations.every(item=>item.animation.cancelled));
});

test('Removing through a replacement rail button cancels the same clip portal',()=>{
  const f=fixture();
  f.button.dataset={favoriteClip:'recording.mkv'};
  animateFavorite(f.button);
  f.button.isConnected=false;
  const replacement=f.button.ownerDocument.createElement();
  replacement.dataset={favoriteClip:'recording.mkv'};replacement.isConnected=true;
  cancelFavoriteBurst(replacement);
  assert.equal(f.root.children.length,0);
  assert.ok(f.animations.every(item=>item.animation.cancelled));
});
