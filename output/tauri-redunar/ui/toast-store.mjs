// Notification lifetime is separate from presentation and native operations.
export function toastStore({changed=()=>{},now=()=>performance.now(),setTimer=setTimeout,clearTimer=clearTimeout,limit=3}={}){
  const entries=[],pauses=new Set();let serial=0;
  function stop(entry){clearTimer(entry.timer);entry.timer=null;entry.generation++;}
  function start(entry){
    stop(entry);if(pauses.size)return;
    entry.deadline=now()+entry.remaining;
    const generation=entry.generation;
    entry.timer=setTimer(()=>{if(entry.generation===generation)remove(entry.id);},entry.remaining);
  }
  function remove(id){
    const index=entries.findIndex(entry=>entry.id===id);if(index<0)return;
    stop(entries[index]);entries.splice(index,1);changed();
  }
  return {
    add(message,{type='info',duration=Math.min(10000,Math.max(5000,String(message).length*35))}={}){
      const existing=entries.find(entry=>entry.message===String(message)&&entry.type===type);
      if(existing){existing.remaining=existing.duration=duration;start(existing);changed();return existing.id;}
      const entry={id:++serial,message:String(message),type,duration,remaining:duration,deadline:0,timer:null,generation:0};
      entries.push(entry);start(entry);
      while(entries.length>limit){stop(entries[0]);entries.shift();}
      changed();return entry.id;
    },
    remove,
    pause(reason){
      if(pauses.has(reason))return;
      if(!pauses.size)for(const entry of entries){entry.remaining=Math.max(0,entry.deadline-now());stop(entry);}
      pauses.add(reason);changed();
    },
    resume(reason){if(!pauses.delete(reason))return;if(!pauses.size)entries.forEach(start);changed();},
    snapshot:()=>entries.map(entry=>({...entry,remaining:pauses.size?entry.remaining:Math.max(0,entry.deadline-now()),paused:pauses.size>0})),
    dispose(){entries.forEach(stop);entries.length=0;pauses.clear();changed();},
  };
}
