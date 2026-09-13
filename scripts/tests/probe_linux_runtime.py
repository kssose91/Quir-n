#!/usr/bin/env python3
"""Prueba aislada en contenedor: /package, /evidence y los dos scripts montados.

Véase docs/ESTABILIZACION_2026-09-10.md. Nunca usa la sesión gráfica del host.
"""
import ctypes as C, ctypes.util, hashlib, json, os, re, struct, subprocess, sys, time, zlib
from pathlib import Path
pkg=Path('/package'); out=Path('/evidence'); out.mkdir(exist_ok=True)
report={'os':Path('/etc/os-release').read_text(),'libc':subprocess.check_output(['getconf','GNU_LIBC_VERSION'],text=True).strip(),'binaries':{},'checks':{}}
for binary in sorted((pkg/'bin').iterdir()):
 p=subprocess.run(['ldd',str(binary)],capture_output=True,text=True)
 report['binaries'][binary.name]={'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'ldd':p.stdout+p.stderr,'exit_code':p.returncode}
 assert p.returncode==0 and 'not found' not in p.stdout+p.stderr,(binary.name,p.stderr)
report['checks']['all_five_binaries_resolve']=len(report['binaries'])==5
help_=subprocess.run([str(pkg/'bin/ledger_admin'),'--help'],capture_output=True,text=True,timeout=20)
assert help_.returncode==0 and 'verify|anchor' in help_.stdout
report['checks']['ledger_admin_executes']=True
if '--gui' in sys.argv:
 import runpy
 runpy.run_path('/installer.py')['require_gui_libraries']()
 report['checks']['dynamic_gui_dependencies']=True
 stage=Path('/tmp/staging')
 run=subprocess.run(['python3',str(pkg/'install.py'),'--destdir',str(stage)],capture_output=True,text=True,timeout=60)
 assert run.returncode==0,run.stderr
 home=stage/Path.home().relative_to('/')
 config=home/'.config/quiron/quiron-brain.env'; before=config.read_bytes()
 assert config.stat().st_mode&0o777==0o600
 assert all((home/'.local/share/quiron/bin'/n).is_file() for n in report['binaries'])
 again=subprocess.run(['python3',str(pkg/'install.py'),'--destdir',str(stage)],capture_output=True,text=True,timeout=60)
 assert again.returncode!=0 and config.read_bytes()==before
 report['checks'].update(installer_staging=True,private_configuration=True,reinstall_preserves_configuration=True)
 runtime=Path('/tmp/runtime');runtime.mkdir(mode=0o700)
 env=dict(os.environ,DISPLAY=':99',XDG_RUNTIME_DIR=str(runtime),XDG_DATA_HOME='/tmp/gui-data',XDG_CONFIG_HOME='/tmp/gui-config')
 env.pop('WAYLAND_DISPLAY',None)
 xvfb_log=(out/'xvfb.log').open('w')
 display=subprocess.Popen(['Xvfb',':99','-screen','0','1280x800x24','-nolisten','tcp'],stdout=xvfb_log,stderr=xvfb_log)
 gui=None
 try:
  time.sleep(1); assert display.poll() is None
  os.environ['DISPLAY']=':99'
  # Reuse the existing reviewed X11 window lookup and PNG capture functions.
  source=Path('/gui-harness.py').read_text()
  exec(source[source.index('x = C.CDLL'):source.index('isolated_data=tempfile.TemporaryDirectory')])
  log=(out/'gui.log').open('w')
  gui=subprocess.Popen([str(pkg/'bin/llore_gui')],env=env,stdout=log,stderr=log,cwd='/tmp')
  window=None
  for _ in range(100):
   assert gui.poll() is None,'GUI exited before creating window'
   window=find(root,gui.pid)
   if window:break
   time.sleep(.2)
  assert window,'No GUI window'
  time.sleep(2);screenshot(window,out/'bienvenida-ubuntu24.png')
  report['checks']['gui_window_rendered']=True
  # Deliver WM_DELETE_WINDOW to this window and check its clean exit.
  class ClientData(C.Union): _fields_=[('b',C.c_char*20),('s',C.c_short*10),('l',C.c_long*5)]
  class ClientMessage(C.Structure): _fields_=[('type',C.c_int),('serial',C.c_ulong),('send_event',C.c_int),('display',Display),('window',Window),('message_type',C.c_ulong),('format',C.c_int),('data',ClientData)]
  class XEvent(C.Union): _fields_=[('client',ClientMessage),('pad',C.c_long*24)]
  event=XEvent();event.client.type=33;event.client.send_event=1;event.client.display=d;event.client.window=window
  event.client.message_type=x.XInternAtom(d,b'WM_PROTOCOLS',0);event.client.format=32;event.client.data.l[0]=x.XInternAtom(d,b'WM_DELETE_WINDOW',0)
  x.XSendEvent.argtypes=[Display,Window,C.c_int,C.c_long,C.POINTER(XEvent)];x.XFlush.argtypes=[Display]
  assert x.XSendEvent(d,window,0,0,C.byref(event));x.XFlush(d)
  code=gui.wait(timeout=15);assert code==0,code
  report['checks']['gui_clean_exit']=True
  x.XCloseDisplay(d);log.close()
 finally:
  if gui and gui.poll() is None:gui.terminate();gui.wait(timeout=10)
  display.terminate();display.wait(timeout=10);xvfb_log.close()
 report['limits']=['Staging and an Xvfb window; no clean-VM Docker/systemd service lifecycle or model download tested here.']
report['ok']=all(report['checks'].values())
(out/('runtime-ubuntu24.json' if '--gui' in sys.argv else 'runtime-debian13.json')).write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'libc':report['libc'],'checks':report['checks'],'ok':report['ok']}))
