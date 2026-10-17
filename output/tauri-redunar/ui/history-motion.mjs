// Presentation only: never resample timestamps, join gaps or change saved data.
// Dense transitions use bounded extrema from both shapes, then restore every
// original point. Inspection always settles first; these are presentation frames.
const POINT_BUDGET = 1024;
function morphIndices(from,to,limit) {
  if(to.length<=limit)return to.map((_,i)=>i);
  const indices=new Set([0,to.length-1]);
  const buckets=Math.max(1,Math.floor((limit-2)/4));
  for(let bucket=0;bucket<buckets;bucket++){
    const start=Math.floor(bucket*to.length/buckets),end=Math.floor((bucket+1)*to.length/buckets);
    for(const points of [from,to]){
      let low=start,high=start;
      for(let i=start+1;i<end;i++){
        if(points[i].y<points[low].y)low=i;
        if(points[i].y>points[high].y)high=i;
      }
      indices.add(low);indices.add(high);
    }
  }
  return [...indices].sort((a,b)=>a-b);
}
export function captureHistoryGraph(svg) {
  if (!svg) return null;
  const lines = [...svg.querySelectorAll('polyline.timeline-trace')];
  if (!lines.length) return null;
  return lines.map(line=>({
    kind: ['primary','cpu','gpu'].find(kind=>line.classList.contains(kind)),
    points: line.historyMotionPoints?.()||Array.from(line.points,point=>({x:point.x,y:point.y})),
    opacity:Number(line.getAttribute('opacity')??1),
    line, exact:line.getAttribute('points'),
    area:line.previousElementSibling?.matches('.timeline-area')?line.previousElementSibling:null,
    areaExact:line.previousElementSibling?.getAttribute('points'),
    areaOpacity:line.previousElementSibling?.matches('.timeline-area')?Number(line.previousElementSibling.getAttribute('opacity')??1):0,
  }));
}

export function historyGraphMotion({
  requestFrame=callback=>requestAnimationFrame(callback),
  cancelFrame=id=>cancelAnimationFrame(id),
  now=()=>performance.now(),
}={}) {
  let pending=null, finish=null;
  function settle() {
    if (pending!==null) cancelFrame(pending);
    pending=null;
    finish?.();finish=null;
  }
  function start(from,svg) {
    settle();
    const to=captureHistoryGraph(svg);
    if (!from||!to) return;
    // Keep the departing secondary trace while it merges/fades into the primary.
    const primary=to.find(target=>target.kind==='primary'),gpu=from.find(source=>source.kind==='gpu');
    if(primary&&gpu&&!to.some(target=>target.kind==='gpu')){
      to.push({...primary,kind:'gpu',line:gpu.line.cloneNode(false),area:null,areaOpacity:0,opacity:0,remove:true});
    }
    if(to.length>POINT_BUDGET/6)return;
    const plans=to.map(target=>{
      // Sensors need not start with frame telemetry. Match a target segment to
      // a subset of one continuous source segment, using actual timestamps only.
      const candidates=[...from.filter(source=>source.kind===target.kind),...from.filter(source=>source.kind!==target.kind)];
      let source;
      for(const candidate of candidates){
        if(candidate.points.length<target.points.length)continue;
        const byTime=new Map(candidate.points.map(point=>[point.x,point]));
        const points=target.points.map(point=>byTime.get(point.x));
        if(points.every(Boolean)){source={...candidate,points};break;}
      }
      if(!source&&target.remove){
        // The departing GPU can fade along its own valid coverage when the
        // primary spans more time; extending it across missing time is unsafe.
        source=gpu;target.points=gpu.points;target.exact=gpu.exact;
      }
      // If coverage differs, fade that exact destination segment in place.
      // A missing sensor must not cancel motion for every other series, nor
      // cause us to join source gaps or manufacture observations.
      source??={...target,opacity:0,areaOpacity:0};
      return {source,target,indices:morphIndices(source.points,target.points,Math.floor(POINT_BUDGET/to.length))};
    });
    let eased=0;
    for(const {source,target} of plans){
      target.opacityExact=target.line.getAttribute('opacity');
      target.opacityFrom=target.kind==='gpu'&&!from.some(line=>line.kind==='gpu')?0:source.opacity;
      if(target.remove){
        target.line.removeAttribute('points');target.line.classList.add('history-motion-exit');
        target.line.setAttribute('aria-hidden','true');svg.append(target.line);
      }
      if(!target.area&&source.area&&source.areaOpacity>0&&target.kind!=='gpu'){
        target.area=source.area.cloneNode(false);target.removeArea=true;
        target.area.classList.add('history-motion-exit');target.line.before(target.area);
      }
      target.areaOpacityExact=target.area?.getAttribute('opacity');
      // A rapid switch captures full current geometry only on demand, not per
      // frame. This keeps a dense session's transition clock within the budget.
      target.line.historyMotionPoints=()=>target.points.map((point,i)=>({x:point.x,y:source.points[i].y+(point.y-source.points[i].y)*eased}));
    }
    const began=now();
    svg.dataset.morphing='true';
    finish=()=>{
      for (const {target} of plans) {
        target.line.setAttribute('points',target.exact);
        delete target.line.historyMotionPoints;
        if(target.opacityExact==null)target.line.removeAttribute('opacity');else target.line.setAttribute('opacity',target.opacityExact);
        if(target.remove)target.line.remove();
        if(target.removeArea)target.area.remove();
        else if(target.area){
          target.area.setAttribute('points',target.areaExact);
          if(target.areaOpacityExact==null)target.area.removeAttribute('opacity');else target.area.setAttribute('opacity',target.areaOpacityExact);
        }
      }
      delete svg.dataset.morphing;
    };
    function paint(progress) {
      eased=1-(1-progress)**3;
      for (const {source,target,indices} of plans) {
        const points=indices.map(i=>`${target.points[i].x},${source.points[i].y+(target.points[i].y-source.points[i].y)*eased}`).join(' ');
        target.line.setAttribute('points',points);
        target.line.setAttribute('opacity',target.opacityFrom+(target.opacity-target.opacityFrom)*eased);
        target.area?.setAttribute('points',`${target.points[0].x},210 ${points} ${target.points.at(-1).x},210`);
        target.area?.setAttribute('opacity',source.areaOpacity+(target.areaOpacity-source.areaOpacity)*eased);
      }
    }
    function frame(time) {
      pending=null;
      if (!svg.isConnected) {settle();return;}
      const progress=Math.min(1,Math.max(0,(time-began)/520));
      if (progress===1) {settle();return;}
      paint(progress);pending=requestFrame(frame);
    }
    paint(0);pending=requestFrame(frame);
  }
  return {start,settle};
}
