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
parser.add_argument('--ask',help='Como --chat, pero la pregunta la hace el editor al abrir (QUIRON_UI_ASK): no depende del teclado del compositor')
parser.add_argument('--stay',type=float,default=0.0,help='Segundos extra con la ventana abierta antes de capturar (p. ej. para ver el índice avanzar)')
parser.add_argument('--enter',action='store_true',help='Pulsa Intro en la bienvenida (Empezar) y captura el programa sin proyecto')
parser.add_argument('--cursor',nargs=2,type=int,metavar=('X','Y'),help='Mueve el cursor a esa posición relativa a la ventana y captura gui-cursor.png')
parser.add_argument('--panel',help='Arranca con una paleta abierta: agentes, agentes:compatible|red|ollama|worker, manual')
parser.add_argument('--consent',action='store_true',help='No saltar la pantalla de permiso del proyecto nuevo (para probarla; --enter acepta)')
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
if args.edit_smoke or args.chat or args.ask:
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
# QUIRON_UI_AUTO_CONSENT: el arnés abre carpetas nuevas sin la pantalla de permiso.
env=dict(os.environ,QUIRON_FRAME_TIMING='1',QUIRON_UI_TRACE='1',QUIRON_UI_AUTO_CONSENT='1',XDG_DATA_HOME=isolated_data.name)
if args.consent: env.pop('QUIRON_UI_AUTO_CONSENT',None)
if args.panel: env['QUIRON_UI_START_PANEL']=args.panel
if args.ask: env['QUIRON_UI_ASK']=args.ask
env.pop('WAYLAND_DISPLAY',None)
log=open(args.output_dir/'gui-smoke.log','w')
arranque_epoch=time.time()
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
    # Encima y con el foco: si hay otra ventana de Quirón (la del usuario),
    # la captura y las teclas deben ir a la de la prueba, no a la que esté arriba.
    subprocess.run(['hyprctl','dispatch','focuswindow','pid:'+str(p.pid)],check=False,stdout=subprocess.DEVNULL)
    subprocess.run(['hyprctl','dispatch','alterzorder','top,pid:'+str(p.pid)],check=False,stdout=subprocess.DEVNULL)
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
def respuesta_guardada(question, desde_epoch):
    # Verdadero cuando el hilo de esta pregunta, creado en esta ronda, tiene ya
    # una respuesta guardada (los hilos viejos con la misma pregunta no cuentan).
    try:
        hilos=json.loads((args.project/'.quiron/state/chats.json').read_text())
    except (OSError, ValueError):
        return False
    hilos=hilos if isinstance(hilos,list) else hilos.get('threads') or []
    for hilo in hilos:
        mensajes=hilo.get('messages') or []
        if hilo.get('created_secs',0) >= desde_epoch and any(m.get('is_user') and m.get('content')==question for m in mensajes):
            return not mensajes[-1].get('is_user')
    return False
def ask_round(question):
    # La pregunta ya salió al abrir el proyecto; solo hay que esperar a que el
    # editor guarde la respuesta en el hilo de esta ronda y capturar.
    inicio_epoch=arranque_epoch-5
    t0=time.monotonic()
    time.sleep(6); step_shot('2-pensando')
    while time.monotonic()-t0<300:
        if respuesta_guardada(question, inicio_epoch):
            time.sleep(0.5); key('Shift_L'); time.sleep(1); step_shot('3-respuesta')
            since=time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(arranque_epoch))
            lines=[l for l in subprocess.run(['journalctl','--user','-u','quiron-brain','--since',since,'-o','cat'],capture_output=True,text=True).stdout.splitlines() if '[gateway]' in l and 'tool_calls=' in l]
            return {'seconds':round(time.monotonic()-t0,1),'llamadas':len(lines),'gateway_log':lines}
        time.sleep(1)
    raise AssertionError('sin respuesta del modelo en 300 s')
def chat_round(question):
    # La respuesta pasa por el cerebro y su gateway; el gateway deja una línea
    # `[claude_cli]` por llamada en el registro del cerebro, y con ella se sabe
    # que la contestación ya está en pantalla.
    since=time.strftime('%Y-%m-%d %H:%M:%S')
    # Al abrir un proyecto el foco queda en el chat: se teclea directamente.
    # (Escape y Tab alternan el foco, y sendshortcut no entrega Ctrl+Shift+P.)
    step_shot('1-abierto')
    # La primera tecla tras el arranque se perdía a veces: una inerte la absorbe.
    subprocess.run(['hyprctl','dispatch','focuswindow','pid:'+str(p.pid)],check=False,stdout=subprocess.DEVNULL); time.sleep(.3)
    key('Shift_L'); time.sleep(.3)
    type_text(question); step_shot('2-pregunta')
    inicio_epoch=time.time()-5
    key('Return')
    t0=time.monotonic()
    while time.monotonic()-t0<300:
        out=subprocess.run(['journalctl','--user','-u','quiron-brain','--since',since,'-o','cat'],capture_output=True,text=True).stdout
        # Cada llamada deja una línea `[gateway] backend=… tool_calls=N` (todos
        # los adaptadores); con manos hay varias y la última es la que no pide
        # herramientas. Las líneas `[claude_cli]` antiguas siguen valiendo.
        lines=[l for l in out.splitlines() if '[gateway]' in l and 'tool_calls=' in l]
        if not lines: lines=[l for l in out.splitlines() if '[claude_cli]' in l and 'tool_calls=' in l]
        if lines and 'tool_calls=0' in lines[-1]:
            # El gateway ya contestó, pero el editor puede reclamar una vez más
            # si el cierre fue de juguete (otra llamada), y la respuesta aún
            # viaja cerebro → editor. La señal firme es la del propio editor:
            # al terminar guarda el hilo en .quiron/state/chats.json.
            if not respuesta_guardada(question, inicio_epoch):
                time.sleep(1); continue
            time.sleep(0.5); key('Shift_L'); time.sleep(1); step_shot('3-respuesta')
            lines=[l for l in subprocess.run(['journalctl','--user','-u','quiron-brain','--since',since,'-o','cat'],capture_output=True,text=True).stdout.splitlines() if '[gateway]' in l and 'tool_calls=' in l]
            return {'seconds':round(time.monotonic()-t0,1),'llamadas':len(lines),'gateway_log':lines}
        time.sleep(1)
    raise AssertionError('sin respuesta del modelo en 300 s')
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
        project=(args.project/'.quiron/project.id').read_text().strip()
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
    if args.ask:
        project=(args.project/'.quiron/project.id').read_text().strip()
        evidence['chat']=dict(question=args.ask,**ask_round(args.ask))
        evidence['index_after_chat']=api('/index/project/'+project)
        screenshot(window,args.output_dir/'gui-chat.png')
        evidence['checks']['chat_answered']=True
        project=None
    if args.chat:
        project=(args.project/'.quiron/project.id').read_text().strip()
        evidence['chat']=dict(question=args.chat,**chat_round(args.chat))
        evidence['index_after_chat']=api('/index/project/'+project)
        screenshot(window,args.output_dir/'gui-chat.png')
        evidence['checks']['chat_answered']=True
        project=None  # el proyecto real no se limpia
    if args.stay > 0:
        time.sleep(args.stay)
    screenshot(window,args.output_dir/'gui-1280.png')
    if args.cursor:
        clientes=json.loads(subprocess.run(['hyprctl','clients','-j'],capture_output=True,text=True).stdout)
        propia=[c for c in clientes if c.get('pid')==p.pid]
        if propia:
            ax,ay=propia[0]['at']
            subprocess.run(['hyprctl','dispatch','movecursor',str(ax+args.cursor[0]),str(ay+args.cursor[1])],check=True,stdout=subprocess.DEVNULL)
            time.sleep(1.5); screenshot(window,args.output_dir/'gui-cursor.png')
    if args.enter:
        key('Return'); time.sleep(1.0)
        screenshot(window,args.output_dir/'gui-programa.png')
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
        assert (args.project/'.quiron/project.id').read_text().strip()==project
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
    if args.chat or args.ask:
        (args.output_dir/'gui-chat.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2)+'\n')
    if fixture: fixture.cleanup()
    isolated_data.cleanup()
