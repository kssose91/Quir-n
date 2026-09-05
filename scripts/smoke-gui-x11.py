#!/usr/bin/env python3
"""Prueba real de ventana bajo Hyprland/XWayland; solo controla su propio proceso."""
import argparse
import tempfile
import json
import shlex
import urllib.request
import ctypes as C, ctypes.util, os, subprocess, time, struct, zlib
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary',type=Path,default=Path(__file__).resolve().parents[1]/'Quirón/llore_editor/target/release/llore_gui')
parser.add_argument('--project',type=Path)
parser.add_argument('--edit-smoke',action='store_true',help='Crear un proyecto temporal y probar abrir, editar, deshacer, guardar y reabrir')
parser.add_argument('--chat',help='Pregunta ASCII que se teclea en el chat con el proyecto abierto; espera la respuesta y la captura')
parser.add_argument('--stay',type=float,default=0.0,help='Segundos extra con la ventana abierta antes de capturar (p. ej. para ver el índice avanzar)')
parser.add_argument('--output-dir',type=Path,required=True)
args=parser.parse_args()
if args.edit_smoke and args.project:
    parser.error('--edit-smoke siempre utiliza su propio proyecto temporal')
if args.chat and not args.project:
    parser.error('--chat necesita --project')
if args.chat and not all(c.isalnum() or c in ' ' for c in args.chat):
    parser.error('--chat solo admite letras, dígitos y espacios (se teclea por atajos)')
args.output_dir.mkdir(parents=True,exist_ok=True)
x = C.CDLL(ctypes.util.find_library('X11'))
Display = C.c_void_p
Window = C.c_ulong
x.XOpenDisplay.argtypes=[C.c_char_p]; x.XOpenDisplay.restype=Display
x.XDefaultRootWindow.argtypes=[Display]; x.XDefaultRootWindow.restype=Window
x.XQueryTree.argtypes=[Display,Window,C.POINTER(Window),C.POINTER(Window),C.POINTER(C.POINTER(Window)),C.POINTER(C.c_uint)]
x.XFetchName.argtypes=[Display,Window,C.POINTER(C.c_void_p)]
x.XFree.argtypes=[C.c_void_p]
x.XInternAtom.argtypes=[Display,C.c_char_p,C.c_int]; x.XInternAtom.restype=C.c_ulong
x.XGetWindowProperty.argtypes=[Display,Window,C.c_ulong,C.c_long,C.c_long,C.c_int,C.c_ulong,C.POINTER(C.c_ulong),C.POINTER(C.c_int),C.POINTER(C.c_ulong),C.POINTER(C.c_ulong),C.POINTER(C.POINTER(C.c_ubyte))]
x.XGetGeometry.argtypes=[Display,Window,C.POINTER(Window),C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint)]
x.XGetImage.argtypes=[Display,Window,C.c_int,C.c_int,C.c_uint,C.c_uint,C.c_ulong,C.c_int]; x.XGetImage.restype=C.c_void_p
x.XGetPixel.argtypes=[C.c_void_p,C.c_int,C.c_int]; x.XGetPixel.restype=C.c_ulong
x.XDestroyImage.argtypes=[C.c_void_p]
x.XResizeWindow.argtypes=[Display,Window,C.c_uint,C.c_uint]
x.XSync.argtypes=[Display,C.c_int]
x.XCloseDisplay.argtypes=[Display]
d=x.XOpenDisplay(None)
if not d: raise SystemExit('No X11 display')
root=x.XDefaultRootWindow(d)
pid_atom=x.XInternAtom(d,b'_NET_WM_PID',0)
def find(parent, pid, depth=0):
    if depth>5: return None
    at=C.c_ulong(); fmt=C.c_int(); n=C.c_ulong(); rem=C.c_ulong(); prop=C.POINTER(C.c_ubyte)()
    x.XGetWindowProperty(d,parent,pid_atom,0,1,0,0,C.byref(at),C.byref(fmt),C.byref(n),C.byref(rem),C.byref(prop))
    match=bool(prop) and n.value and fmt.value==32 and C.cast(prop,C.POINTER(C.c_ulong))[0]==pid
    if prop: x.XFree(prop)
    if match: return parent
    r=Window(); p=Window(); children=C.POINTER(Window)(); count=C.c_uint()
    if not x.XQueryTree(d,parent,C.byref(r),C.byref(p),C.byref(children),C.byref(count)): return None
    values=[children[i] for i in range(count.value)]
    if children: x.XFree(children)
    for child in values:
        found=find(child,pid,depth+1)
        if found: return found
    return None

def screenshot(w,path):
    r=Window(); a=C.c_int(); b=C.c_int(); width=C.c_uint(); height=C.c_uint(); border=C.c_uint(); depth=C.c_uint()
    x.XGetGeometry(d,w,C.byref(r),C.byref(a),C.byref(b),C.byref(width),C.byref(height),C.byref(border),C.byref(depth))
    image=x.XGetImage(d,w,0,0,width.value,height.value,C.c_ulong(-1).value,2)
    if not image: raise RuntimeError('XGetImage failed')
    rows=bytearray()
    for y in range(height.value):
        rows.append(0)
        for col in range(width.value):
            v=x.XGetPixel(image,col,y)
            rows.extend(((v>>16)&255,(v>>8)&255,v&255))
    x.XDestroyImage(image)
    def chunk(t,data): return struct.pack('!I',len(data))+t+data+struct.pack('!I',zlib.crc32(t+data)&0xffffffff)
    png=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!IIBBBBB',width.value,height.value,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(rows))+chunk(b'IEND',b'')
    Path(path).write_bytes(png)
    print(path, width.value, height.value)

isolated_data=tempfile.TemporaryDirectory(prefix='quiron-gui-data-')
fixture=tempfile.TemporaryDirectory(prefix='quiron-gui-edit-') if args.edit_smoke else None
source=None
if fixture:
    args.project=Path(fixture.name)
    source=args.project/'math.rs'
    source.write_text('pub fn answer() -> u32 { 42 }\n// ')
values={}
if args.edit_smoke or args.chat:
    config=Path(os.environ.get('QUIRON_BRAIN_ENV_FILE',Path.home()/'.config/quiron/quiron-brain.env'))
    for line in config.read_text().splitlines():
        if '=' in line and not line.lstrip().startswith('#'):
            k,v=line.split('=',1);values[k]=shlex.split(v)[0] if v.strip() else ''
def api(path,method=None):
    req=urllib.request.Request('http://127.0.0.1:'+values.get('QUIRON_PORT','8766')+path,
        method=method,headers={'Authorization':'Bearer '+values['QUIRON_API_TOKEN']})
    with urllib.request.urlopen(req,timeout=30) as r:
        body=r.read()
        return json.loads(body) if body else None
def indexed(project,predicate):
    deadline=time.monotonic()+120
    while time.monotonic()<deadline:
        progress=api('/index/project/'+project)
        if progress['phase']=='watching' and predicate(progress): return progress
        time.sleep(1)
    raise AssertionError(progress)
env=dict(os.environ,QUIRON_FRAME_TIMING='1',XDG_DATA_HOME=isolated_data.name)
env.pop('WAYLAND_DISPLAY',None)
log=open(args.output_dir/'gui-smoke.log','w')
p=subprocess.Popen([str(args.binary.resolve())]+([str(args.project.resolve())] if args.project else []),env=env,stdout=log,stderr=log)
def own_window():
    window=None
    for _ in range(50):
        if p.poll() is not None: raise RuntimeError('GUI exited early: '+str(p.returncode))
        window=find(root,p.pid)
        if window: break
        time.sleep(.2)
    if not window: raise RuntimeError('No window for GUI process')
    subprocess.run(['hyprctl','dispatch','setfloating','pid:'+str(p.pid)],check=True,stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl','dispatch','resizewindowpixel','exact 1280 720,pid:'+str(p.pid)],check=True,stdout=subprocess.DEVNULL)
    time.sleep(12 if args.project else 2)
    return window
def key(key,mods=''):
    if p.poll() is not None: raise RuntimeError('El proceso de prueba ya terminó')
    result=subprocess.run(['hyprctl','dispatch','sendshortcut',f'{mods},{key},pid:{p.pid}'],check=True,capture_output=True,text=True)
    if result.stdout.strip()!='ok': raise RuntimeError(result.stdout)
    time.sleep(.08)
def open_math():
    key('p','CTRL');time.sleep(.4)
    for char in 'math': key(char)
    key('Return');time.sleep(.5)
def type_text(text):
    for char in text:
        key('space' if char==' ' else char)
def step_shot(name):
    # Con SMOKE_STEP_SHOTS=1 se captura cada paso, para ver qué hizo cada tecla.
    if os.environ.get('SMOKE_STEP_SHOTS'):
        time.sleep(.4); screenshot(find(root,p.pid),args.output_dir/f'paso-{name}.png')
def chat_round(question):
    # La respuesta pasa por el cerebro y su gateway; el gateway deja una línea
    # `[claude_cli]` por llamada en el registro del cerebro, y con ella se sabe
    # que la contestación ya está en pantalla.
    since=time.strftime('%Y-%m-%d %H:%M:%S')
    # Al abrir un proyecto el foco queda en el chat: se teclea directamente.
    # (Escape y Tab alternan el foco, y sendshortcut no entrega Ctrl+Shift+P.)
    step_shot('1-abierto')
    # La primera tecla tras el arranque se perdía a veces: una inerte la absorbe.
    key('Shift_L'); time.sleep(.3)
    type_text(question); step_shot('2-pregunta')
    key('Return')
    t0=time.monotonic()
    while time.monotonic()-t0<300:
        out=subprocess.run(['journalctl','--user','-u','quiron-brain','--since',since,'-o','cat'],capture_output=True,text=True).stdout
        lines=[l for l in out.splitlines() if '[claude_cli]' in l and 'model=' in l]
        # Con manos hay varias llamadas: la última es la que no pide herramientas.
        if lines and 'tool_calls=0' in lines[-1]:
            # La respuesta aún viaja gateway → cerebro → editor; y el editor solo
            # repinta ante un evento: una tecla inerte lo provoca.
            time.sleep(3); key('Shift_L'); time.sleep(1); step_shot('3-respuesta')
            return {'seconds':round(time.monotonic()-t0,1),'llamadas':len(lines),'gateway_log':lines}
        time.sleep(1)
    raise AssertionError('sin respuesta del modelo en 180 s')
def close():
    subprocess.run(['hyprctl','dispatch','closewindow','pid:'+str(p.pid)],check=True,stdout=subprocess.DEVNULL)
    try:
        code=p.wait(timeout=8)
    except subprocess.TimeoutExpired:
        # Se captura la ventana tal como quedó: la barra de estado dice por qué
        # no cerró (p. ej. pestañas sin guardar).
        w=find(root,p.pid)
        if w: screenshot(w,args.output_dir/'cierre-bloqueado.png')
        raise
    assert code==0, code
    return code
project=None
evidence={'checks':{}}
try:
    window=own_window()
    if args.edit_smoke:
        project=(args.project/'.llore/project.id').read_text().strip()
        before=indexed(project,lambda s:s['files_total']==1)
        open_math()
        key('End','CTRL')
        for char in 'edited': key(char)
        key('z','CTRL');key('z','CTRL SHIFT')
        key('s','CTRL');time.sleep(.5)
        assert source.read_text()=='pub fn answer() -> u32 { 42 }\n// edited', repr(source.read_text())
        after=indexed(project,lambda s:s['summaries_generated']>before['summaries_generated'])
        evidence['checks'].update(open_edit_undo_redo_save=True, saved_change_indexed=True)
        evidence.update(project_id=project,before=before,after_save=after)
    if args.chat:
        project=(args.project/'.llore/project.id').read_text().strip()
        evidence['chat']=dict(question=args.chat,**chat_round(args.chat))
        evidence['index_after_chat']=api('/index/project/'+project)
        screenshot(window,args.output_dir/'gui-chat.png')
        evidence['checks']['chat_answered']=True
        project=None  # el proyecto real no se limpia
    if args.stay > 0:
        time.sleep(args.stay)
    screenshot(window,args.output_dir/'gui-1280.png')
    subprocess.run(['hyprctl','dispatch','resizewindowpixel','exact 900 600,pid:'+str(p.pid)],check=True,stdout=subprocess.DEVNULL); time.sleep(.5)
    screenshot(window,args.output_dir/'gui-900.png')
    print('GUI exit:',close())
    if args.edit_smoke:
        assert api('/index/project/'+project)['phase']=='watching'
        p=subprocess.Popen([str(args.binary.resolve()),str(args.project)],env=env,stdout=log,stderr=log)
        window=own_window();open_math();key('End','CTRL')
        for char in 'again': key(char)
        key('s','CTRL');time.sleep(.5)
        assert source.read_text()=='pub fn answer() -> u32 { 42 }\n// editedagain', repr(source.read_text())
        assert (args.project/'.llore/project.id').read_text().strip()==project
        evidence['after_reopen']=indexed(project,lambda s:s['summaries_generated']>after['summaries_generated'])
        screenshot(window,args.output_dir/'gui-reopened.png')
        close()
        evidence['checks'].update(reopen_preserves_saved_content=True, project_identity_stable=True, monitor_survives_window_close=True)
        evidence['ok']=True
finally:
    if p.poll() is None:
        p.terminate(); p.wait(timeout=5)
    log.close(); x.XCloseDisplay(d)
    if project:
        try:
            source.unlink(missing_ok=True)
            indexed(project,lambda s:s['files_total']==0)
            api('/index/project/'+project,method='DELETE')
        except Exception as error:
            evidence['cleanup_error']=str(error)
    if args.edit_smoke:
        (args.output_dir/'gui-edit.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2)+'\n')
    if args.chat:
        (args.output_dir/'gui-chat.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2)+'\n')
    if fixture: fixture.cleanup()
    isolated_data.cleanup()
