// Closed-form damped motion is independent of display refresh rate. Retargeting
// keeps both the current position and velocity; only one frame is queued.
export function springStep(position,velocity,target,seconds,{stiffness=625,damping=50}={}) {
  const offset=position-target,half=damping/2,discriminant=stiffness-half*half;
  if(Math.abs(discriminant)<1e-7){
    const decay=Math.exp(-half*seconds),slope=velocity+half*offset;
    return {position:target+(offset+slope*seconds)*decay,velocity:(velocity-half*slope*seconds)*decay};
  }
  if(discriminant>0){
    const frequency=Math.sqrt(discriminant),slope=(velocity+half*offset)/frequency;
    const angle=frequency*seconds,cos=Math.cos(angle),sin=Math.sin(angle),decay=Math.exp(-half*seconds);
    return {position:target+decay*(offset*cos+slope*sin),velocity:decay*((slope*frequency-half*offset)*cos-(offset*frequency+half*slope)*sin)};
  }
  const spread=Math.sqrt(-discriminant),a=-half+spread,b=-half-spread;
  const first=(velocity-b*offset)/(a-b),second=offset-first;
  return {position:target+first*Math.exp(a*seconds)+second*Math.exp(b*seconds),velocity:a*first*Math.exp(a*seconds)+b*second*Math.exp(b*seconds)};
}

export function springMotion(initial,render,{
  stiffness=625,damping=50,epsilon=.04,velocityEpsilon=.7,maxDuration=1000,
  now=()=>performance.now(),requestFrame=fn=>requestAnimationFrame(fn),cancelFrame=id=>cancelAnimationFrame(id),onRest=()=>{},
}={}) {
  const keys=Object.keys(initial);
  let value={...initial},velocity=Object.fromEntries(keys.map(key=>[key,0])),target={...initial};
  let frame=null,generation=0,last=now(),started=last,paint=render;
  const advance=time=>{
    const dt=Math.max(0,(time-last)/1000);last=Math.max(last,time);
    for(const key of keys){const next=springStep(value[key],velocity[key],target[key],dt,{stiffness,damping});value[key]=next.position;velocity[key]=next.velocity;}
  };
  const resting=()=>keys.every(key=>Math.abs(value[key]-target[key])<epsilon&&Math.abs(velocity[key])<velocityEpsilon);
  function stop(){if(frame!==null)cancelFrame(frame);frame=null;generation++;}
  function finish(){stop();value={...target};velocity=Object.fromEntries(keys.map(key=>[key,0]));paint({...value});onRest();}
  function queue(){
    const token=generation;
    frame=requestFrame(time=>{
      if(token!==generation)return;
      advance(time);
      if(time-started>=maxDuration||resting()){finish();return;}
      paint({...value});
      if(token===generation)queue();
    });
  }
  paint({...value});
  return {
    to(next,{immediate=false}={}){
      if(!immediate&&keys.every(key=>next[key]===target[key]))return;
      const time=now();if(frame!==null)advance(time);else last=time;
      target={...next};started=time;
      if(immediate||resting()){finish();return;}
      if(frame===null)queue();
    },
    bind(nextRender){if(frame!==null)advance(now());paint=nextRender;paint({...value});},
    snapshot(){if(frame!==null)advance(now());return {value:{...value},velocity:{...velocity},target:{...target}};},
    get running(){return frame!==null;},
    finish,stop,
  };
}
