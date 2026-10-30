"""Check restored native wheel response with real input on an explicitly private X display."""
import ctypes
import os

if os.environ.get('REDUNAR_PRIVATE_DESKTOP_FIXTURE') != '1':
    raise SystemExit('Run under xvfb-run with REDUNAR_PRIVATE_DESKTOP_FIXTURE=1')

# Native input is sent to DISPLAY; do not let GTK select the host Wayland socket.
os.environ['GDK_BACKEND']='x11'

from webview_fixture import *

extra="""
bridge.idle=true;bridge.automaticUpdates=false;
window.appWheelListeners=0;
const addListener=EventTarget.prototype.addEventListener;
EventTarget.prototype.addEventListener=function(type,...args){
  if(type==='wheel')appWheelListeners++;
  return addListener.call(this,type,...args);
};
"""
manager.add_script(WebKit2.UserScript.new(extra,WebKit2.UserContentInjectedFrames.TOP_FRAME,WebKit2.UserScriptInjectionTime.START,None,None))
x11=ctypes.CDLL('libX11.so.6');xtest=ctypes.CDLL('libXtst.so.6')
x11.XOpenDisplay.argtypes=[ctypes.c_char_p];x11.XOpenDisplay.restype=ctypes.c_void_p
x11.XFlush.argtypes=[ctypes.c_void_p];x11.XCloseDisplay.argtypes=[ctypes.c_void_p]
xtest.XTestFakeMotionEvent.argtypes=[ctypes.c_void_p,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_ulong]
xtest.XTestFakeButtonEvent.argtypes=[ctypes.c_void_p,ctypes.c_uint,ctypes.c_int,ctypes.c_ulong]
display=x11.XOpenDisplay(None)
if not display:
    raise RuntimeError('Private X display unavailable')

def wheel(button):
    assert xtest.XTestFakeMotionEvent(display,-1,1000,500,0)
    for pressed in (1,0):
        assert xtest.XTestFakeButtonEvent(display,button,pressed,0)
    x11.XFlush(display)

gtk_settings=Gtk.Settings.get_default()
original_animations=gtk_settings.get_property('gtk-enable-animations')
gtk_settings.set_property('gtk-enable-animations',True)
try:
    assert view.get_settings().get_enable_smooth_scrolling()
    view.load_uri(f'http://127.0.0.1:{server.server_port}/#overview');pump(1200)
    wait_for("qa('.recent-thumbnail img').length===3&&!matchMedia('(prefers-reduced-motion: reduce)').matches")
    test('The production UI registers no wheel interception',"check(appWheelListeners===0,'wheel listeners: '+appWheelListeners);")
    js("""
    window.root=q('.window-content');window.positions=[];window.extent=root.scrollHeight;
    root.addEventListener('scroll',()=>positions.push(root.scrollTop),{passive:true});
    window.frames=0;const originalFrame=window.requestAnimationFrame;
    window.requestAnimationFrame=callback=>{frames++;return originalFrame(callback);};
    window.syntheticWheel=(target,values={})=>{
      const event=new WheelEvent('wheel',{deltaY:99,deltaMode:0,bubbles:true,cancelable:true,...values});
      Object.defineProperty(event,'wheelDeltaY',{value:values.legacy??-120});
      target.dispatchEvent(event);return event.defaultPrevented;
    };
    """)
    wheel(5);pump(500)
    test('A real native notch has intermediate positions and retains distance',"""
    check(positions.length>1,'native intermediate frames: '+JSON.stringify(positions));
    check(positions[0]>0&&positions[0]<99,'native smooth start');
    check(root.scrollTop===99,'native notch distance: '+root.scrollTop);
    check(root.scrollHeight===extent,'page extent stable');positions=[];
    """)
    wheel(4);pump(500)
    test('Native reverse input returns to the top',"check(positions.length>1,'reverse intermediate frames');check(root.scrollTop===0,'reverse distance');positions=[];")
    for _ in range(5):
        wheel(5);pump(25)
    pump(500)
    test('Native bursts retain wheel distance',"check(root.scrollTop===495,'burst distance: '+root.scrollTop);window.settledPositions=positions.length;")
    pump(300)
    test('Native scrolling adds no JavaScript animation loop',"check(frames===0,'unexpected RAF callbacks: '+frames);check(positions.length===settledPositions,'native scrolling has settled');")
    test('Coarse, fine, horizontal and modifier gestures remain native',"""
    for(const values of [{},{deltaY:2,legacy:-2},{deltaY:44.5},{deltaX:4},{ctrlKey:true},{shiftKey:true},{metaKey:true},{altKey:true}])
      check(!syntheticWheel(q('.recent-capture-grid'),values),'native gesture: '+JSON.stringify(values));
    const input=document.createElement('input');root.append(input);
    check(!syntheticWheel(input),'form control wheel retained');input.remove();
    const nested=document.createElement('div');nested.style.cssText='height:50px;overflow-y:auto';
    nested.innerHTML='<div style="height:300px">nested fixture</div>';root.append(nested);
    check(!syntheticWheel(nested.firstChild),'nested scroller retained');
    nested.scrollTop=250;check(!syntheticWheel(nested.firstChild),'nested boundary retained');nested.remove();
    """)
    gtk_settings.set_property('gtk-enable-animations',False)
    wait_for("matchMedia('(prefers-reduced-motion: reduce)').matches")
    test('Reduced motion keeps native event ownership',"check(!syntheticWheel(q('.recent-capture-grid')),'reduced-motion native handling');root.scrollTop=0;")
    pump(100);wheel(5);pump(500)
    test('Native wheel input works with reduced motion',"check(root.scrollTop===99,'native distance');check(frames===0,'no JS animation');")
    gtk_settings.set_property('gtk-enable-animations',True)
    wait_for("!matchMedia('(prefers-reduced-motion: reduce)').matches")
    route('global');pump(250)
    test('Navigation retains the new page starting position',"check(root.scrollTop===0,'new route starts at top');check(bridge.errors.length===0,bridge.errors.join('; '));")
    route('overview');pump(250)
    test('Wheel input leaves thumbnails and geometry intact',"check(qa('.recent-thumbnail img').length===3,'loaded thumbnails retained');check(root.scrollHeight===extent,'original page extent retained');")
    snap('overview-native-scroll-restored')
    (ARTIFACTS/'overview-wheel-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
    print(f'PASS: {len(checks)} native wheel checks.')
finally:
    gtk_settings.set_property('gtk-enable-animations',original_animations)
    x11.XCloseDisplay(display);window.destroy();server.shutdown()
