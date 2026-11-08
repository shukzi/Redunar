// Short choice groups retain their native select as the form and value owner.
const controls=new WeakMap();
const unavailable=option=>option.disabled||(option.parentElement?.tagName==='OPTGROUP'&&option.parentElement.disabled);

export function nextChoiceIndex(disabled,current,key) {
 const enabled=disabled.map((off,index)=>off?-1:index).filter(index=>index>=0);
 if(!enabled.length)return -1;
 if(key==='Home')return enabled[0];
 if(key==='End')return enabled.at(-1);
 const delta=({ArrowLeft:-1,ArrowRight:1,ArrowUp:-1,ArrowDown:1})[key];
 if(delta===undefined)return null;
 for(let n=1;n<=disabled.length;n++){
  const index=((current+delta*n)%disabled.length+disabled.length)%disabled.length;
  if(!disabled[index])return index;
 }
 return current;
}

function signature(select){return [...select.options].map(option=>JSON.stringify([option.value,option.textContent,unavailable(option)])).join('|');}
function sync(select,control) {
 const next=signature(select),disabled=select.matches(':disabled');
 let changed=control.signature!==next;
 if(changed){
  control.wrapper.querySelectorAll(':scope>button').forEach(button=>button.remove());
  [...select.options].forEach((option,index)=>{
   const button=select.ownerDocument.createElement('button');
   button.type='button';button.textContent=option.textContent;button.dataset.choiceIndex=index;
   control.wrapper.insertBefore(button,control.wrapper.querySelector(':scope>.selection-indicator'));
  });
  control.signature=next;
 }
 for(const button of control.wrapper.querySelectorAll(':scope>button')){
  const index=Number(button.dataset.choiceIndex),selected=index===select.selectedIndex;
  const pressed=String(selected),off=disabled||unavailable(select.options[index]);
  if(button.getAttribute('aria-pressed')!==pressed){button.setAttribute('aria-pressed',pressed);changed=true;}
  if(button.disabled!==off){button.disabled=off;changed=true;}
  button.tabIndex=selected&&!off?0:-1;
 }
 if(!control.wrapper.querySelector('button[tabindex="0"]')){
  const first=control.wrapper.querySelector('button:not(:disabled)');if(first)first.tabIndex=0;
 }
 control.wrapper.setAttribute('aria-label',control.label);
 const description=select.getAttribute('aria-describedby');
 if(description)control.wrapper.setAttribute('aria-describedby',description);
 if(changed)control.wrapper.dispatchEvent(new Event('choice-switcher-sync',{bubbles:true}));
}

function replacement(select,label) {
 if(select.isConnected)return select;
 return [...document.querySelectorAll('select[data-choice-switcher]')].find(candidate=>select.id?candidate.id===select.id:candidate.getAttribute('aria-label')===label);
}

function commit(select,index,control) {
 const option=select.options[index];
 if(!option||select.matches(':disabled')||unavailable(option))return;
 const changed=select.selectedIndex!==index;
 select.selectedIndex=index;sync(select,control);
 if(changed)select.dispatchEvent(new Event('change',{bubbles:true}));
 const current=replacement(select,control.label);
 if(current){enhanceChoiceSwitcher(current,control.label);focusChoiceSwitcher(current);}
}

export function enhanceChoiceSwitcher(select,label) {
 let control=controls.get(select);
 if(!control){
  const wrapper=select.ownerDocument.createElement('span');
  wrapper.className='choice-switcher';wrapper.dataset.choiceGroup=select.id||label;
  wrapper.setAttribute('role','group');
  const wasFocused=select.ownerDocument.activeElement===select;
  select.before(wrapper);wrapper.append(select);select.hidden=true;select.tabIndex=-1;
  control={wrapper,label,signature:null,typed:'',lastType:0};controls.set(select,control);
  wrapper.addEventListener('click',event=>{
   const button=event.target.closest('button[data-choice-index]');
   if(!button||button.parentElement!==wrapper)return;
   event.preventDefault();commit(select,Number(button.dataset.choiceIndex),control);
  });
  wrapper.addEventListener('keydown',event=>{
   const button=event.target.closest('button[data-choice-index]');
   if(!button||button.disabled)return;
   let index=nextChoiceIndex([...select.options].map(unavailable),Number(button.dataset.choiceIndex),event.key);
   if(index===null&&event.key.length===1&&event.key!==' '&&!event.ctrlKey&&!event.altKey&&!event.metaKey){
    const now=Date.now();control.typed=now-control.lastType>700?event.key.toLowerCase():control.typed+event.key.toLowerCase();control.lastType=now;
    index=[...select.options].findIndex(option=>!unavailable(option)&&option.textContent.toLowerCase().startsWith(control.typed));
   }
   if(index===null)return;
   event.preventDefault();if(index>=0)commit(select,index,control);
  });
  sync(select,control);if(wasFocused)focusChoiceSwitcher(select);return;
 }
 control.label=label;sync(select,control);
}

export function focusChoiceSwitcher(select) {
 const control=controls.get(select);if(!control)return;
 (control.wrapper.querySelector('button[aria-pressed="true"]:not(:disabled)')||control.wrapper.querySelector('button:not(:disabled)'))?.focus({preventScroll:true});
}
