import test from 'node:test';
import assert from 'node:assert/strict';
import { refreshSteamSetup } from '../ui/steam-setup.mjs';

const setup = {available:true,configured:false,configuration_state:'not-configured',launch_options:'/usr/bin/redunar-steam-launch --app-id 42 -- %command%'};
function panel() {
  const content = {innerHTML:'initial'}, button = {disabled:false};
  return {isConnected:true,content,button,querySelector:selector=>selector.includes('content')?content:button};
}
function deferred() {
  let resolve;
  const promise = new Promise(done=>{resolve=done;});
  return {promise,resolve};
}

test('a detached game panel cannot receive late setup evidence', async () => {
  const view=panel(), pending=deferred();
  const request=refreshSteamSetup(view,'1',()=>pending.promise);
  view.isConnected=false;
  pending.resolve(setup);
  await request;
  assert.equal(view.content.innerHTML,'initial');
});

test('an older check cannot overwrite a newer configured response', async () => {
  const view=panel(), pending=deferred();
  const old=refreshSteamSetup(view,'1',()=>pending.promise);
  await refreshSteamSetup(view,'1',async(command,args)=>{
    assert.equal(command,'steam_setup_status');
    assert.deepEqual(args,{gameId:'1'});
    return {...setup,configured:true};
  });
  const newer=view.content.innerHTML;
  pending.resolve(setup);
  await old;
  assert.equal(view.content.innerHTML,newer);
  assert.match(newer,/Launch options configured/);
  assert.equal(view.button.disabled,false);
});

test('failed checks show escaped errors and permit a successful retry', async () => {
  const view=panel();
  await refreshSteamSetup(view,'1',async()=>{throw Error('<unavailable>');});
  assert.match(view.content.innerHTML,/&lt;unavailable&gt;/);
  assert.equal(view.button.disabled,false);
  await refreshSteamSetup(view,'1',async()=>({...setup,launch_options:'"/fixture/a & b/wrapper" --app-id 42 -- %command%'}));
  assert.match(view.content.innerHTML,/Setup required/);
  assert.match(view.content.innerHTML,/&quot;\/fixture\/a &amp; b\/wrapper&quot;/);
});

test('unavailable setup never offers an unusable launch option', async () => {
  const view=panel();
  await refreshSteamSetup(view,'1',async()=>({...setup,available:false,status:'Wrapper missing'}));
  assert.match(view.content.innerHTML,/Wrapper missing/);
  assert.doesNotMatch(view.content.innerHTML,/textarea|data-copy-steam-options/);
});

test('running Steam gives a launch step without turning ambiguity into success', async () => {
  const view=panel();
  await refreshSteamSetup(view,'1',async()=>({...setup,configuration_state:'steam-running',status:'Needs attention: Steam is running, so its in-memory Launch Options may differ from the saved file'}));
  assert.match(view.content.innerHTML,/Launch option not yet confirmed/);
  assert.match(view.content.innerHTML,/click Play in Steam to test the connection/);
  assert.match(view.content.innerHTML,/data-copy-steam-options/);
  assert.doesNotMatch(view.content.innerHTML,/Needs attention|in-memory|saved file|before it is saved|Launch options configured/);
});

test('other ambiguous configuration reasons keep their explanation', async () => {
  const view=panel();
  await refreshSteamSetup(view,'1',async()=>({...setup,configuration_state:'needs-attention',status:'Steam user accounts do not agree on this game’s Launch Options'}));
  assert.match(view.content.innerHTML,/Setup unconfirmed/);
  assert.match(view.content.innerHTML,/Steam user accounts do not agree/);
  assert.doesNotMatch(view.content.innerHTML,/Steam is open|test the connection|before it is saved/);
});

test('unchanged setup evidence preserves the existing option field', async () => {
  const view=panel();
  await refreshSteamSetup(view,'1',async()=>setup);
  const markup=view.content.innerHTML;
  let writes=0;
  Object.defineProperty(view.content,'innerHTML',{get:()=>markup,set:()=>{writes++;}});
  await refreshSteamSetup(view,'1',async()=>setup);
  assert.equal(writes,0);
});
