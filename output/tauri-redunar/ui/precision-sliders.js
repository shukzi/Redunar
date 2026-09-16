// Native range semantics handle keyboard, pointer, min/max, and disabled states.
export function updatePrecisionSlider(input) {
  if(!input||input.classList.contains('trim-range'))return;
  const min=Number(input.min||0),max=Number(input.max||100);
  const fill=max>min?(Number(input.value)-min)/(max-min)*100:0;
  input.style.setProperty('--range-fill',`${Math.max(0,Math.min(100,fill))}%`);
}
export function updatePrecisionSliders(root=document) {
  root.querySelectorAll('input[type=range]:not(.trim-range)').forEach(updatePrecisionSlider);
}
document.addEventListener('input',event=>{
  if(event.target.matches('input[type=range]'))updatePrecisionSlider(event.target);
},true);
