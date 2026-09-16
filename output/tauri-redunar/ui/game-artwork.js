// Two requests at a time, bounded per-window cache, and no list replacement.
// Local cache misses are normal for launcher-agnostic games.
const cache=new Map(), pending=new Map(), inflight=new Set();
let active=0;
const CACHE_BYTES=12*1024*1024;
let cachedBytes=0;
function remember(id,url){
 cachedBytes-=(cache.get(id)?.length||0)*2;cache.delete(id);cache.set(id,url);cachedBytes+=(url?.length||0)*2;
 while(cache.size>64||cachedBytes>CACHE_BYTES){const first=cache.keys().next().value;cachedBytes-=(cache.get(first)?.length||0)*2;cache.delete(first);}
}
const artworkNodes=root=>root.querySelectorAll('[data-poster],[data-banner]');
const identity=node=>{const kind=node.hasAttribute('data-banner')?'banner':'poster';const id=node.dataset[kind];return {kind,id,key:`${kind}:${id}`};};
function paint(root,key,url){
  for(const node of artworkNodes(root)){
    if(identity(node).key!==key||!url||node.querySelector('img'))continue;
    const img=document.createElement('img');img.alt='';img.loading='lazy';img.decoding='async';img.src=url;
    const fallback=[...node.childNodes].map(child=>child.cloneNode(true));
    img.addEventListener('error',()=>{remember(key,null);node.replaceChildren(...fallback);},{once:true});
    node.replaceChildren(img);
  }
}
function drain(){
  while(active<2&&pending.size){
    const [key,{root,call,id,kind}]=pending.entries().next().value;pending.delete(key);
    if(!root.isConnected)continue;
    active++;inflight.add(key);
    call(kind==='banner'?'game_banner':'game_poster',{gameId:id}).then(bytes=>{
      let url=null;
      if(Array.isArray(bytes)&&bytes.length>0&&bytes.length<=1048576){
        let binary='';for(let i=0;i<bytes.length;i+=8192)binary+=String.fromCharCode(...bytes.slice(i,i+8192));
        url=`data:${bytes[0]===137?'image/png':'image/jpeg'};base64,${btoa(binary)}`;
      }
      remember(key,url);
      paint(root,key,url);
    }).catch(()=>{}).finally(()=>{active--;inflight.delete(key);drain();});
  }
}
export function loadGameArtwork(root,call){
  for(const node of artworkNodes(root)){
    const {id,kind,key}=identity(node);
    if(cache.has(key))paint(root,key,cache.get(key));
    else if(!inflight.has(key)&&pending.size<64)pending.set(key,{root,call,id,kind});
  }
  drain();
}
