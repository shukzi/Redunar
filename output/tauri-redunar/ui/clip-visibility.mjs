// Let the engine report visibility instead of synchronously measuring every
// saved card during scrolling. Inventory and preview queues retain their owners.
export function clipVisibilityTracker({changed,makeObserver=(callback,options)=>new IntersectionObserver(callback,options)}) {
 let observer=null,generation=0,order=new Map(),visible=new Set();
 return {
  bind(rail) {
   const current=++generation;
   observer?.disconnect();observer=null;order.clear();visible.clear();
   if(!rail)return;
   for(const [index,node] of [...rail.querySelectorAll('[data-clip]')].entries())order.set(node,index);
   if(!order.size)return;
   // The viewport root also respects clipping by a nested scrolling rail. It
   // continues to work when responsive CSS makes that rail part of page scroll.
   observer=makeObserver(entries=>{
    if(current!==generation)return;
    let updated=false;
    for(const entry of entries){
     const node=entry.target;if(!order.has(node))continue;
     // At threshold zero WebKit can report a touching edge as intersecting
     // with zero area, then send no second entry as it moves fully inside.
     // Prefetch that adjacent card once instead of leaving its preview stuck.
     const shown=node.isConnected&&entry.isIntersecting;
     if(shown&&!visible.has(node)){visible.add(node);updated=true;}
     else if(!shown&&visible.delete(node))updated=true;
    }
    if(updated)changed();
   },{root:null,threshold:0});
   for(const node of order.keys())observer.observe(node);
  },
  ids() {
   return [...visible].filter(node=>node.isConnected).sort((a,b)=>order.get(a)-order.get(b)).map(node=>node.dataset.clip);
  },
 };
}
