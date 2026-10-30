import {test} from 'node:test';
import assert from 'node:assert/strict';
import {setDisclosureOpen} from '../ui/disclosure-motion.mjs';

function fixture({open=false,floating=false,reduced=false}={}) {
 const animations=[];
 const animate=(frames,options)=>{
  const animation={frames,options,cancelled:false,onfinish:null,cancel(){this.cancelled=true;}};
  animations.push(animation);return animation;
 };
 const content={inert:false,animate};
 const summary={nextElementSibling:content,getBoundingClientRect:()=>({height:40})};
 const details={open,dataset:{},style:{height:'',overflow:'',boxSizing:''},animate,
  querySelector:()=>summary,
  getBoundingClientRect(){return {height:this.paintedHeight??(this.open?200:40)};},
  ownerDocument:{defaultView:{matchMedia:()=>({matches:reduced}),getComputedStyle:element=>element===content?{position:floating?'absolute':'static',opacity:'0.5',clipPath:'inset(0 0 30% 0)',transform:'none'}:{paddingTop:'0',paddingBottom:'0',borderTopWidth:'0',borderBottomWidth:'0'}}}
 };
 return {details,content,animations};
}

test('rapid reversal settles at the latest choice and ignores a cancelled finish',()=>{
 const {details,content,animations}=fixture();
 setDisclosureOpen(details,true);
 const staleFinish=animations[0].onfinish;
 details.paintedHeight=90;
 setDisclosureOpen(details,false);
 assert.equal(animations[0].cancelled,true);
 assert.equal(animations[1].frames[0].height,'90px');
 assert.equal(content.inert,true);
 staleFinish();
 assert.equal(details.open,true,'closing content remains rendered until the latest animation completes');
 animations[1].onfinish();
 assert.equal(details.open,false);
 assert.deepEqual(details.style,{height:'',overflow:'',boxSizing:''});
 assert.equal(content.inert,false);
 assert.equal(details.dataset.expanded,undefined);
});

test('immediate reveal cancels a pending collapse without a late close',()=>{
 const {details,animations}=fixture({open:true});
 setDisclosureOpen(details,false);
 const staleFinish=animations[0].onfinish;
 setDisclosureOpen(details,true,{immediate:true});
 staleFinish();
 assert.equal(details.open,true);
 assert.equal(animations[0].cancelled,true);
 assert.equal(details.style.overflow,'');
});

test('reduced motion changes the native state without scheduling animation',()=>{
 const {details,animations}=fixture({reduced:true});
 setDisclosureOpen(details,true);
 assert.equal(details.open,true);
 setDisclosureOpen(details,false);
 assert.equal(details.open,false);
 assert.equal(animations.length,0);
});

test('floating menus animate their content without changing the header height',()=>{
 const {details,content,animations}=fixture({floating:true});
 setDisclosureOpen(details,true);
 assert.equal(details.style.height,'');
 assert.equal(animations[0].frames[0].opacity,0);
 animations[0].onfinish();
 assert.equal(details.open,true);
 setDisclosureOpen(details,false);
 assert.equal(content.inert,true);
 animations[1].onfinish();
 assert.equal(details.open,false);
 assert.equal(content.inert,false);
});
