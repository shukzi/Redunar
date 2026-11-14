import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

function fixture() {
 const messages=[];
 const context={createLinearGradient:()=>({addColorStop(){}}),fillRect(){},fillText(){}};
 const page={URLSearchParams,structuredClone,atob,location:{search:''},addEventListener(){},
  document:{createElement:()=>({getContext:()=>context,toDataURL:()=> 'data:image/png;base64,AA=='})},
  webkit:{messageHandlers:{fixture:{postMessage:message=>messages.push(JSON.parse(message))}}}};
 page.window=page;
 vm.createContext(page);
 vm.runInContext(readFileSync(new URL('./native-fixture.js',import.meta.url),'utf8'),page);
 return {page,messages,invoke:page.__TAURI_INTERNALS__.invoke};
}

test('Unknown commands reject and remain visible even when the caller catches the error',async()=>{
 const {page,messages,invoke}=fixture();
 await assert.rejects(invoke('catalog_typo'),/Unexpected native fixture command/);
 assert.equal(page.bridge.unexpectedCommands.length,1);
 assert.equal(messages[0].command,'catalog_typo');
 assert.equal(page.bridge.calls[0].command,'catalog_typo');
});

test('Malformed arguments fail before a handler can acknowledge them',async()=>{
 const {messages,invoke}=fixture();
 for(const args of [null,[],{enabled:'yes'}])await assert.rejects(invoke('set_close_to_tray',args),/Invalid/);
 await assert.rejects(invoke('save_global_settings',{input:null}),/Invalid input/);
 assert.equal(messages.length,4);
 const valid=await invoke('set_close_to_tray',{enabled:false});
 assert.equal(valid.close_to_tray,false);
});

test('Known void calls succeed explicitly and playback reports its unavailable state',async()=>{
 const {messages,invoke}=fixture();
 assert.equal(await invoke('plugin:window|minimize'),null);
 assert.equal(await invoke('cancel_clip_playback'),null);
 await assert.rejects(invoke('clip_playback_path',{fileName:'fixture.mkv'}),/Fixture playback preparation unavailable/);
 assert.equal(messages.length,0);
});

test('Named scenario handlers can delay completion without another native read',async()=>{
 const {page,invoke}=fixture();
 let release;
 page.bridge.setHandler('catalog_games',()=>new Promise(resolve=>{release=resolve;}));
 const pending=invoke('catalog_games');
 assert.equal(page.bridge.calls.length,1);
 release([{id:'fixture'}]);
 assert.deepEqual(await pending,[{id:'fixture'}]);
});

test('Default shortcut and save handlers retain the existing synthetic states',async()=>{
 const {invoke}=fixture();
 assert.equal((await invoke('activate_shortcuts')).state,'Active');
 assert.deepEqual(JSON.parse(JSON.stringify(await invoke('save_replay'))),{});
});
