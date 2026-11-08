// Lucide artwork versions: lucide-artwork.json. ISC and Feather MIT notices:
// packaging/LICENSE-Lucide.txt.
// SVG geometry is upstream artwork; sizing, rendering and motion are local.
import artwork from './lucide-artwork.json' with {type:'json'};

const aliases = {
  overview:'group', library:'library-big', global:'sliders-horizontal', replay:'clapperboard',
  history:'rotate-ccw', refresh:'refresh-cw', arrow:'arrow-right',
  speaker:'volume-2', speakerMuted:'volume-x', shieldCheck:'shield-check',
  warning:'triangle-exclamation-point', success:'circle-check', pending:'clock-3',
  spinner:'loader-circle', restart:'refresh-cw', standard:'file-text', confirm:'save',
  video:'clapperboard', square:'maximize', sidebar:'panel-left-close',
};
const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));

export function icon(name, className='') {
  const key=aliases[name]||name;
  const kind=Object.hasOwn(artwork.icons,key)?key:aliases.global;
  let strokes=artwork.icons[kind];
  // The four original corner paths move as one frame, independently of the blocks.
  if(kind==='group'){const split=strokes.indexOf('<rect');strokes=`<g class="group-frame">${strokes.slice(0,split)}</g>${strokes.slice(split)}`;}
  // Separate the continuous upstream outline at the shared bottom, keeping
  // its geometry intact while the front flap moves independently of the back.
  if(kind==='folder-open')strokes='<path d="M18.45 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/><path d="M6 14l1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5"/>';
  const fill=className.split(/\s+/).includes('clip-favorite-icon')?`<g class="favorite-icon-fill" stroke="none">${artwork.icons[kind]}</g>`:'';
  return `<svg class="icon ${escape(className)}" data-lucide="${kind}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${fill}<g class="icon-strokes" fill="none">${strokes}</g></svg>`;
}

export function mountShellIcons(root=document) {
  root.querySelectorAll('[data-ui-icon]').forEach(slot=>{
    slot.outerHTML=icon(slot.dataset.uiIcon,slot.className);
  });
  // Keep the panel outline fixed. Only the chevron changes direction or moves.
  root.querySelectorAll('.sidebar-collapse-icon .icon-strokes').forEach(group=>{
    if(group.querySelector('.sidebar-chevron-motion'))return;
    const chevron=group.lastElementChild;
    const motion=group.ownerDocument.createElementNS('http://www.w3.org/2000/svg','g');
    motion.classList.add('sidebar-chevron-motion');chevron.classList.add('sidebar-chevron');
    chevron.before(motion);motion.append(chevron);
  });
}
