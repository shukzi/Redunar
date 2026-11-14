// Keep one transient message and one expiry timer. Deadlines never pause.
export function toastStore({changed=()=>{},now=()=>performance.now(),wallNow=()=>Date.now(),setTimer=setTimeout,clearTimer=clearTimeout}={}){
  let entry=null,timer=null,serial=0,generation=0,reading=false,disposed=false;
  function stop(){if(timer!==null)clearTimer(timer);timer=null;generation++;}
  // Wall time covers system sleep. Monotonic time bounds backward clock changes.
  function remaining(){return Math.max(0,Math.min(entry.deadline-now(),entry.wallDeadline-wallNow()));}
  function expire(){
    if(!entry||reading||remaining()>0)return;
    stop();entry=null;changed();
  }
  function schedule(){
    stop();if(!entry||reading||disposed)return;
    if(remaining()<=0){expire();return;}
    const token=generation;
    timer=setTimer(()=>{if(token!==generation||disposed)return;timer=null;expire();if(entry)schedule();},remaining());
  }
  return {
    add(message,{type='info',duration}={}){
      if(disposed)return;
      const text=String(message??'').trim();if(!text)return;
      const kind=['info','success','warning','error'].includes(type)?type:'info';
      const base=kind==='error'?5000:kind==='warning'?4000:kind==='success'?2000:2500;
      const fallback=Math.min(6000,Math.max(base,text.length*30));
      const lifetime=Number.isFinite(duration)?Math.min(6000,Math.max(1000,duration)):fallback;
      const id=entry?.message===text&&entry.type===kind?entry.id:++serial;
      entry={id,message:text,type:kind,duration:lifetime,deadline:now()+lifetime,wallDeadline:wallNow()+lifetime};
      schedule();changed();return id;
    },
    remove(id){if(entry?.id!==id)return;stop();entry=null;changed();},
    // Reading can defer removal, but cannot move the original expiry time.
    hold(value){if(disposed||reading===value)return;reading=value;expire();schedule();},
    expire,
    snapshot:()=>entry?[{...entry,remaining:remaining()}]:[],
    dispose(){if(disposed)return;disposed=true;stop();entry=null;reading=false;changed();},
  };
}
