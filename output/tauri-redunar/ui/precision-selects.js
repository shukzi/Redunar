import {icon} from './icons.mjs';
// Preserve each native select as the value/form owner. The Precision control
// presents its options and emits the same change event the existing UI handles.
import {enhanceChoiceSwitcher,focusChoiceSwitcher} from './choice-switchers.mjs';
import {setPopupExpanded,cancelPopupMotion} from './popup-motion.mjs';
const controls = new WeakMap();
const retiring = new Map();
let popup = null;
let serial = 0;
const reducedCommitMotion = matchMedia('(prefers-reduced-motion: reduce)');
const commitAnimations = new Map();
reducedCommitMotion.addEventListener('change',()=>{
  if(!reducedCommitMotion.matches)return;
  for(const animation of commitAnimations.values())animation.cancel();
  commitAnimations.clear();
});
function animateValueCommit(value) {
  commitAnimations.get(value)?.cancel();
  commitAnimations.delete(value);
  if(reducedCommitMotion.matches||document.documentElement.dataset.page==='replay-menu')return;
  // Restart feedback without forcing layout to remove/re-add a CSS animation.
  const animation=value.animate([{opacity:.5,transform:'translateY(2px)'},{opacity:1,transform:'none'}],{duration:150,easing:'cubic-bezier(.2,.8,.2,1)'});
  animation.id='control-value-commit';
  commitAnimations.set(value,animation);
  animation.onfinish=animation.oncancel=()=>{if(commitAnimations.get(value)===animation)commitAnimations.delete(value);};
}
const chevron = icon('chevron-down');
function labelFor(select) {
  if(select.getAttribute('aria-label'))return select.getAttribute('aria-label');
  const labelledBy=select.getAttribute('aria-labelledby');
  if(labelledBy)return labelledBy.split(/\s+/).map(id=>document.getElementById(id)?.textContent||'').join(' ').trim();
  const label=select.labels?.[0];
  if(label){const copy=label.cloneNode(true);copy.querySelectorAll('select,.precision-select-trigger').forEach(el=>el.remove());return copy.textContent.trim();}
  return select.closest('.field')?.querySelector('strong')?.textContent||'Choose an option';
}
function disabled(option) { return option.disabled || (option.parentElement.tagName==='OPTGROUP'&&option.parentElement.disabled); }
export function closePrecisionSelect({immediate=true}={}) {
  if(!popup)return;
  const state=popup;
  popup=null;
  state.trigger.setAttribute('aria-expanded','false');
  state.trigger.removeAttribute('aria-activedescendant');
  state.list.inert=true;state.list.setAttribute('aria-hidden','true');
  state.list.classList.remove('precision-select-menu');
  state.list.classList.add('precision-select-exit');
  retiring.set(state.select,state);
  setPopupExpanded(state.list,false,{edge:state.edge,immediate,onfinish:()=>{
    if(retiring.get(state.select)!==state)return;
    retiring.delete(state.select);state.list.remove();
  }});
}
const dismiss=()=>closePrecisionSelect({immediate:false});
function dismissAll(){
  closePrecisionSelect();
  for(const state of retiring.values()){cancelPopupMotion(state.list);state.list.remove();}
  retiring.clear();
}
function sync(select,control) {
  const text=select.selectedOptions[0]?.textContent||'Choose an option';
  // Preserve the mounted text node when a status poll retains the value.
  if(control.value.textContent!==text)control.value.textContent=text;
  const unavailable=select.matches(':disabled');
  if(control.trigger.disabled!==unavailable)control.trigger.disabled=unavailable;
  const label=labelFor(select);
  if(control.trigger.getAttribute('aria-label')!==label)control.trigger.setAttribute('aria-label',label);
  if(control.link&&control.link.hidden!==(select.value!=='inherit'))control.link.hidden=select.value!=='inherit';
  const description=select.getAttribute('aria-describedby');
  if(description&&control.trigger.getAttribute('aria-describedby')!==description)control.trigger.setAttribute('aria-describedby',description);
  if(popup?.select===select&&(control.trigger.disabled||popup.signature!==signature(select)))closePrecisionSelect();
}
function signature(select){return [...select.options].map(o=>[o.value,o.textContent,disabled(o),o.selected].join(':')).join('|');}
export function enhancePrecisionSelects(root=document) {
  if(popup&&!popup.select.isConnected)closePrecisionSelect();
  root.querySelectorAll('select:not([multiple])').forEach(select=>{
    if(select.size>1)return;
    if(select.hasAttribute('data-choice-switcher')){enhanceChoiceSwitcher(select,labelFor(select));return;}
    let control=controls.get(select);
    if(!control){
      const wrapper=document.createElement('span');wrapper.className='precision-select';
      const trigger=document.createElement('button');trigger.type='button';trigger.className='precision-select-trigger';
      trigger.setAttribute('role','combobox');trigger.setAttribute('aria-haspopup','listbox');trigger.setAttribute('aria-expanded','false');
      const id=`precision-options-${++serial}`;trigger.setAttribute('aria-controls',id);
      const value=document.createElement('span');value.className='precision-select-value';
      let link=null;
      if(select.hasAttribute('data-override')){link=document.createElement('span');link.className='inheritance-link';link.setAttribute('aria-hidden','true');trigger.append(link);}
      trigger.append(value);trigger.insertAdjacentHTML('beforeend',chevron);
      const wasFocused=document.activeElement===select;
      select.before(wrapper);wrapper.append(select,trigger);
      select.hidden=true;select.tabIndex=-1;
      control={trigger,value,id,link};controls.set(select,control);
      trigger.addEventListener('click',()=>{if(popup?.select===select)dismiss();else open(select);});
      trigger.addEventListener('keydown',event=>onKey(event,select));
      if(wasFocused)trigger.focus({preventScroll:true});
    }
    sync(select,control);
  });
}
export function focusPrecisionSelect(select) {
  if(!select)return;
  enhancePrecisionSelects(select.parentElement);
  if(select.hasAttribute('data-choice-switcher')){focusChoiceSwitcher(select);return;}
  (controls.get(select)?.trigger||select).focus({preventScroll:true});
}
function highlight(index) {
  if(!popup||index<0||popup.active===index)return;
  popup.active=index;
  const {list,trigger}=popup,item=list.children[index];
  // Active styling changes only color. Read the final option/viewport geometry
  // together before changing classes, inherited plate properties or scroll.
  const geometry={x:item.offsetLeft,y:item.offsetTop,w:item.offsetWidth,h:item.offsetHeight};
  const top=list.scrollTop,height=list.clientHeight;
  const nextTop=geometry.y<top?geometry.y:geometry.y+geometry.h>top+height?geometry.y+geometry.h-height:top;
  [...list.children].forEach((item,i)=>item.classList.toggle('active',i===index));
  for(const [key,value] of Object.entries(geometry))list.style.setProperty(`--select-active-${key}`,`${value}px`);
  list.style.setProperty('--select-active-visible',item.getAttribute('aria-selected')==='true'?'0':'1');
  trigger.setAttribute('aria-activedescendant',item.id);
  // Scroll the popup only, never the document or clip rail.
  if(nextTop!==top)list.scrollTop=nextTop;
}
function open(select) {
  dismiss();
  const control=controls.get(select),options=[...select.options];
  if(!control||select.matches(':disabled')||!options.length)return;
  sync(select,control);
  const closing=retiring.get(select);
  if(closing){
    retiring.delete(select);
    if(closing.signature===signature(select)&&closing.list.isConnected){
      popup=closing;popup.active=-1;popup.typed='';popup.lastType=0;
      closing.list.inert=false;closing.list.removeAttribute('aria-hidden');
      closing.list.classList.replace('precision-select-exit','precision-select-menu');
      control.trigger.setAttribute('aria-expanded','true');
      highlight(select.selectedIndex>=0&&!disabled(options[select.selectedIndex])?select.selectedIndex:options.findIndex(o=>!disabled(o)));
      setPopupExpanded(closing.list,true,{edge:closing.edge});
      return;
    }
    cancelPopupMotion(closing.list);closing.list.remove();
  }
  const list=document.createElement('ul');list.className='precision-select-menu';list.id=control.id;list.dataset.focusPlate='';list.setAttribute('role','listbox');list.setAttribute('aria-label',labelFor(select));
  list.dataset.selectSurface='';
  options.forEach((option,index)=>{
    const item=document.createElement('li');item.className='precision-select-option';item.id=`${control.id}-${index}`;
    item.setAttribute('role','option');item.setAttribute('aria-selected',String(option.selected));item.setAttribute('aria-disabled',String(disabled(option)));
    const text=document.createElement('span');text.textContent=option.textContent;
    if(select.hasAttribute('data-override')&&option.value==='inherit'){const link=document.createElement('span');link.className='inheritance-link';link.setAttribute('aria-hidden','true');item.classList.add('inherited-option');item.append(link);}
    const check=document.createElement('span');check.className='precision-select-check';check.innerHTML=icon('check');check.setAttribute('aria-hidden','true');item.append(text,check);
    item.addEventListener('pointerdown',event=>event.preventDefault());
    item.addEventListener('click',()=>{if(popup?.list===list)commit(index);});
    item.addEventListener('pointermove',()=>{if(popup?.list===list&&!disabled(option))highlight(index);});
    list.append(item);
  });
  const rect=control.trigger.getBoundingClientRect(),margin=8;
  const width=Math.min(Math.max(rect.width,200),innerWidth-margin*2);
  list.style.width=`${width}px`;list.style.left=`${Math.max(margin,Math.min(rect.right-width,innerWidth-width-margin))}px`;
  (select.closest('dialog')||document.body).append(list);
  popup={select,trigger:control.trigger,list,active:-1,typed:'',lastType:0,signature:signature(select)};
  const contentHeight=list.scrollHeight,inset=list.offsetHeight-list.clientHeight;
  const below=innerHeight-rect.bottom-margin,above=rect.top-margin;
  const up=below<Math.min(contentHeight,300)&&above>below;
  popup.edge=up?'bottom':'top';
  const maxHeight=Math.max(40,Math.min(300,(up?above:below)-margin));
  list.style.maxHeight=`${maxHeight}px`;
  if(up)list.style.bottom=`${innerHeight-rect.top+margin}px`;else list.style.top=`${rect.bottom+margin}px`;
  // Decide overflow from the measured content and final height before the
  // highlight pass reads geometry with the final scrollbar gutter in place.
  list.classList.toggle('precision-select-scrolls',contentHeight>maxHeight-inset+1);
  control.trigger.setAttribute('aria-expanded','true');
  highlight(select.selectedIndex>=0&&!disabled(options[select.selectedIndex])?select.selectedIndex:options.findIndex(o=>!disabled(o)));
  setPopupExpanded(list,true,{edge:popup.edge});
}
function commit(index) {
  if(!popup)return;
  const {select}=popup,option=select.options[index];
  if(!option||select.matches(':disabled')||disabled(option))return;
  const label=labelFor(select),id=select.id,changed=select.selectedIndex!==index;
  dismiss();select.selectedIndex=index;
  sync(select,controls.get(select));
  animateValueCommit(controls.get(select).value);
  if(changed)select.dispatchEvent(new Event('change',{bubbles:true}));
  // A settings change may rebuild the field. Restore focus to its replacement.
  enhancePrecisionSelects();
  const replacement=select.isConnected?select:[...document.querySelectorAll('select')].find(el=>id?el.id===id:labelFor(el)===label);
  focusPrecisionSelect(replacement);
}
function onKey(event,select) {
  if(select.matches(':disabled'))return;
  if(event.key==='Tab'){dismiss();return;}
  if(event.key==='Escape'){if(popup){event.preventDefault();event.stopPropagation();dismiss();}return;}
  if(event.key==='Enter'||event.key===' '){event.preventDefault();if(popup?.select===select)commit(popup.active);else open(select);return;}
  const enabled=[...select.options].map((o,i)=>disabled(o)?-1:i).filter(i=>i>=0);
  if(!enabled.length)return;
  if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
    event.preventDefault();
    if(popup?.select!==select){open(select);if(event.key.startsWith('Arrow'))return;}
    const pos=enabled.indexOf(popup.active);
    highlight(event.key==='Home'?enabled[0]:event.key==='End'?enabled.at(-1):enabled[(pos+(event.key==='ArrowDown'?1:-1)+enabled.length)%enabled.length]);
  }else if(event.key.length===1&&!event.ctrlKey&&!event.metaKey&&!event.altKey){
    event.preventDefault();if(popup?.select!==select)open(select);
    const now=Date.now();popup.typed=now-popup.lastType>700?event.key.toLowerCase():popup.typed+event.key.toLowerCase();popup.lastType=now;
    const match=enabled.find(i=>select.options[i].textContent.toLowerCase().startsWith(popup.typed));if(match!==undefined)highlight(match);
  }
}
document.addEventListener('pointerdown',event=>{if(popup&&!popup.list.contains(event.target)&&!popup.trigger.contains(event.target))dismiss();});
document.addEventListener('focusin',event=>{if(popup&&!popup.list.contains(event.target)&&event.target!==popup.trigger)dismiss();});
document.addEventListener('change',event=>{if(event.target.matches('select'))queueMicrotask(()=>enhancePrecisionSelects());});
// Fixed menus close on page scroll/resize rather than drifting away from a field.
document.addEventListener('scroll',event=>{if(event.target!==popup?.list&&!(event.target instanceof Element&&event.target.closest('.precision-select-exit')))dismissAll();},true);
window.addEventListener('resize',dismissAll);
window.addEventListener('hashchange',dismissAll);
window.addEventListener('pagehide',dismissAll);
document.addEventListener('close',dismissAll,true);
