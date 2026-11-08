import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizedSliderValue,commitSliderValue,updatePrecisionSlider} from '../ui/precision-sliders.js';

test('exact slider values respect each range and its supported steps',()=>{
  assert.equal(normalizedSliderValue('97',50,200,5),95);
  assert.equal(normalizedSliderValue('999',50,200,5),200);
  assert.equal(normalizedSliderValue('-4',0,100,1),0);
  assert.equal(normalizedSliderValue(' 73,6 ',0,100,1),74);
  assert.equal(normalizedSliderValue('-11.8',-12,24,1),-12);
  assert.equal(normalizedSliderValue('+12',-12,24,1),12);
  for(const value of ['', '-', '12px', 'NaN', 'Infinity', '1e999'])assert.equal(normalizedSliderValue(value,0,100,1),null);
});

function controls(initial='50') {
  const events=[],attributes=new Map(),styles=new Map();
  const field={value:initial,disabled:false,nextElementSibling:{textContent:'%'},closest:()=>group};
  const range={value:initial,min:'0',max:'100',step:'1',disabled:false,
    classList:{contains:()=>false},style:{setProperty:(key,value)=>styles.set(key,value)},
    ownerDocument:{activeElement:field,defaultView:{Event}},
    closest:()=>group,hasAttribute:key=>attributes.has(key),getAttribute:key=>attributes.get(key),setAttribute:(key,value)=>attributes.set(key,value),
    dispatchEvent:event=>events.push(event.type)};
  const group={querySelector:selector=>selector==='[data-slider-value]'?field:range};
  return {field,range,events,styles};
}

test('committing an edit sends one native range event pair and repeated blur does not save twice',()=>{
  const {field,range,events}=controls();field.value='73.6';
  commitSliderValue(field);
  assert.equal(range.value,'74');assert.equal(field.value,'74');
  assert.deepEqual(events,['input','change']);
  commitSliderValue(field);assert.deepEqual(events,['input','change']);
});

test('unfinished and disabled edits leave the saved range alone',()=>{
  const {field,range,events}=controls();field.value='-';commitSliderValue(field);
  assert.equal(field.value,'50');assert.equal(range.value,'50');
  range.disabled=true;field.value='90';commitSliderValue(field);
  assert.equal(field.value,'50');assert.equal(field.disabled,true);assert.deepEqual(events,[]);
});

test('polling preserves an unfinished value edit and later synchronizes the displayed value',()=>{
  const {field,range}=controls();field.value='-';updatePrecisionSlider(range);
  assert.equal(field.value,'-');
  range.ownerDocument.activeElement=null;range.value='62';updatePrecisionSlider(range);
  assert.equal(field.value,'62');assert.equal(range.getAttribute('aria-valuetext'),'62 percent');
});

test('an authoritative save result restores the displayed value even when the field retains focus',()=>{
  const {field,range}=controls();field.value='74';
  updatePrecisionSlider(range,{forceValue:true});
  assert.equal(field.value,'50');
});

test('tactile fill follows the native thumb center at minimum, middle and maximum',()=>{
  const {range,styles}=controls();range.setAttribute('data-tactile-range','');
  range.min='50';range.max='200';
  for(const [value,fraction] of [[50,0],[125,.5],[200,1]]){
    range.value=String(value);updatePrecisionSlider(range);
    assert.equal(styles.get('--range-center'),`calc(9px + (100% - 18px) * ${fraction})`);
    assert.equal(styles.get('--range-fill'),`${fraction*100}%`);
  }
});
