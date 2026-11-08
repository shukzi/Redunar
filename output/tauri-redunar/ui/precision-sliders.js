// Native ranges own pointer/keyboard input. Exact-value fields commit through
// those same events, keeping draft and recording-save behavior in their owners.
const escapeAttribute=value=>String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
export function sliderValueMarkup({id,label,value,unit='%'}) {
  return `<span class="slider-value"><input id="${escapeAttribute(id)}" type="text" inputmode="decimal" autocomplete="off" spellcheck="false" data-slider-value aria-label="${escapeAttribute(label)}, ${unit==='%'?'percent':'decibels'}" value="${escapeAttribute(value)}"><span aria-hidden="true">${escapeAttribute(unit)}</span></span>`;
}

export function normalizedSliderValue(raw,min,max,step) {
  const text=String(raw).trim().replace(',','.');
  if(!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)$/.test(text))return null;
  const value=Number(text),lower=Number(min),upper=Number(max),increment=Number(step);
  if(!Number.isFinite(value)||!Number.isFinite(lower)||!Number.isFinite(upper)||upper<lower)return null;
  const bounded=Math.max(lower,Math.min(upper,value));
  const snapped=Number.isFinite(increment)&&increment>0?lower+Math.round((bounded-lower)/increment)*increment:bounded;
  return Number(Math.max(lower,Math.min(upper,snapped)).toFixed(8));
}

function sliderValueInput(input) {
  return input.closest('[data-slider-control]')?.querySelector('[data-slider-value]');
}
export function updatePrecisionSlider(input,{forceValue=false}={}) {
  if(!input||input.classList.contains('trim-range'))return;
  const min=Number(input.min||0),max=Number(input.max||100);
  const fill=max>min?(Number(input.value)-min)/(max-min)*100:0;
  input.style.setProperty('--range-fill',`${Math.max(0,Math.min(100,fill))}%`);
  if(input.hasAttribute('data-tactile-range'))input.style.setProperty('--range-center',`calc(9px + (100% - 18px) * ${Math.max(0,Math.min(1,fill/100))})`);
  const value=sliderValueInput(input);
  if(value){
    if(value.disabled!==input.disabled)value.disabled=input.disabled;
    if((forceValue||input.ownerDocument.activeElement!==value)&&value.value!==input.value)value.value=input.value;
    const unit=value.nextElementSibling?.textContent==='%'?'percent':'decibels';
    const label=`${input.value} ${unit}`;
    if(input.getAttribute('aria-valuetext')!==label)input.setAttribute('aria-valuetext',label);
  }
}

export function commitSliderValue(value) {
  const input=value.closest('[data-slider-control]')?.querySelector('input[type="range"]');
  if(!input)return;
  const next=input.disabled?null:normalizedSliderValue(value.value,input.min,input.max,input.step);
  const previous=input.value;
  if(next!==null)input.value=String(next);
  value.value=input.value;
  updatePrecisionSlider(input);
  if(input.value===previous)return;
  const Event=input.ownerDocument.defaultView.Event;
  input.dispatchEvent(new Event('input',{bubbles:true}));
  input.dispatchEvent(new Event('change',{bubbles:true}));
}
export function updatePrecisionSliders(root=document) {
  root.querySelectorAll('input[type=range]:not(.trim-range)').forEach(input=>updatePrecisionSlider(input));
}
if(typeof document!=='undefined'){
  document.addEventListener('input',event=>{
    if(event.target.matches('input[type=range]'))updatePrecisionSlider(event.target);
  },true);
  document.addEventListener('focusout',event=>{
    if(event.target.matches('[data-slider-value]'))commitSliderValue(event.target);
  });
  document.addEventListener('keydown',event=>{
    const value=event.target;
    if(!value.matches('[data-slider-value]'))return;
    if(event.key==='Enter'){
      event.preventDefault();commitSliderValue(value);
    }else if(event.key==='Escape'){
      event.preventDefault();event.stopPropagation();
      const input=value.closest('[data-slider-control]')?.querySelector('input[type="range"]');
      if(input)value.value=input.value;
    }
  });
}
