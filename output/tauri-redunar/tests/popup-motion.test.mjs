import {test} from 'node:test';
import assert from 'node:assert/strict';
import {setPopupExpanded} from '../ui/popup-motion.mjs';

function fixture(){
  const animations=[],handlers={},preference={matches:false,addEventListener:(name,fn)=>handlers.preference=fn};
  const view={matchMedia:()=>preference,addEventListener:(name,fn)=>handlers[name]=fn,getComputedStyle:()=>({opacity:'.42',transform:'matrix(1,0,0,.98,0,-1)'})};
  const panel={ownerDocument:{defaultView:view},style:{transformOrigin:''},animate(frames,timing){
    const animation={frames,timing,onfinish:null,cancelled:false,cancel(){this.cancelled=true;}};
    animations.push(animation);return animation;
  }};
  return {panel,animations,handlers,preference};
}
test('a reversed popup starts from its painted frame and ignores stale dismissal',()=>{
  const {panel,animations}=fixture();let removed=0;
  setPopupExpanded(panel,false,{onfinish:()=>removed++});
  const stale=animations[0].onfinish;
  setPopupExpanded(panel,true);
  assert.equal(animations[0].cancelled,true);
  assert.deepEqual(animations[1].frames[0],{opacity:'.42',transform:'matrix(1,0,0,.98,0,-1)'});
  stale();assert.equal(removed,0);
  animations[1].onfinish();assert.equal(panel.style.transformOrigin,'');
});
test('upward menus fold toward their lower anchor without moving the final layout',()=>{
  const {panel,animations}=fixture();
  setPopupExpanded(panel,true,{edge:'bottom'});
  assert.equal(panel.style.transformOrigin,'center bottom');
  assert.match(animations[0].frames[0].transform,/translateY\(2px\)/);
  animations[0].onfinish();assert.equal(panel.style.transformOrigin,'');
});
test('reduced motion settles a closing popup once and restores temporary styles',()=>{
  const {panel,animations,handlers,preference}=fixture();let removed=0;
  setPopupExpanded(panel,false,{onfinish:()=>removed++});
  const stale=animations[0].onfinish;
  preference.matches=true;handlers.preference();stale();
  assert.equal(removed,1);assert.equal(panel.style.transformOrigin,'');
  setPopupExpanded(panel,false,{onfinish:()=>removed++});
  assert.equal(removed,2);assert.equal(animations.length,1);
});
