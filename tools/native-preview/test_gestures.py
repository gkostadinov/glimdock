#!/usr/bin/env python3
"""Exercise the production UI through continuous LVGL pointer input, without hardware.

The preference adapter uses only an in-memory map. No device settings, Wi-Fi
credentials, serial ports or remote endpoints are accessed by this test.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from render import ROOT, configured_rotation, viewport, fixture, png

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'output/native-gestures')
    source_key=hashlib.sha256(str(ROOT.resolve()).encode()).hexdigest()[:10]
    parser.add_argument('--build-dir',type=Path,default=Path(tempfile.gettempdir())/('homelab-native-gesture-build-'+source_key))
    parser.add_argument("--rotation",type=int,choices=range(4),default=configured_rotation(),help="0/2 portrait, 1/3 landscape; defaults to firmware config")
    args=parser.parse_args()
    args.build_dir = args.build_dir.with_name(args.build_dir.name+"-r"+str(args.rotation))
    width,height=viewport(args.rotation)
    args.output.mkdir(parents=True,exist_ok=True);args.build_dir.mkdir(parents=True,exist_ok=True)
    document=json.loads((ROOT/'examples/snapshots/firmware-inventory.json').read_text())
    document['node']={'id':'test:pve','type':'proxmox','name':'Test lab','address':'192.0.2.10','status':'healthy'}
    document['nodes']=[document['node'],{'id':'test:printer','type':'klipper','name':'Test printer','address':'192.0.2.30:7125','status':'healthy'}]
    # Explicit UI test data: no external guest/device probe is performed.
    gib=1073741824
    if document.get('guests'):
        g=document['guests'][0];g.update(memory_basis='guest-os',mem_assigned_bytes=g.get('mem_total_bytes'),mem_host_bytes=8*gib,pve_mem_used_bytes=8*gib,pve_mem_total_bytes=8*gib,mem_age_s=4)
    if len(document.get('guests',[]))>1:
        document['guests'][1].update(memory_basis='guest-os-arc',mem_used_bytes=14.8*gib,mem_total_bytes=15.6*gib,mem_available_bytes=.8*gib,mem_cache_bytes=12.6*gib,mem_noncache_used_bytes=2.2*gib,mem_assigned_bytes=16*gib,mem_host_bytes=16.1*gib,pve_mem_used_bytes=16.1*gib,pve_mem_total_bytes=16*gib,mem_age_s=4)
    owner='vm:'+str(document['guests'][0]['id'])
    document['gpus']=[{'id':'test:discrete','name':'Test discrete GPU','vendor':'NVIDIA','kind':'discrete','owner':owner,'driver':'nvidia','status':'active','utilization_pct':37,'mem_used_bytes':2*gib,'mem_total_bytes':16*gib,'temp_c':52,'power_w':110,'graphics_mhz':1400,'memory_mhz':1150,'fan_pct':0,'age_s':4}, {'id':'test:integrated','name':'Test integrated GPU','vendor':'Intel','kind':'integrated','owner':'host','driver':'i915','status':'partial','utilization_kind':'busiest-engine','utilization_pct':0,'graphics_mhz':0,'age_s':4,'error':'Optional temperature, power and shared-memory counters unavailable'}]
    document['sensors'] += [{'id':'test:package-w','name':'CPU package','chip':'Intel RAPL / turbostat','kind':'power','value':93.4,'unit':'W'}, {'id':'test:gpu-w','name':'NVIDIA GPU','chip':'NVIDIA / nvidia-smi','kind':'power','value':25.1,'unit':'W'}]
    fingerprint=fixture(document,args.build_dir/'fixture.h')
    printer={'schema':1,'demo':True,'generated_at':document.get('generated_at'),'node':document['nodes'][1],'nodes':document['nodes'],'sources':{'moonraker':{'enabled':True,'ok':True,'age_s':1}},'printer':{'id':'test:printer','klippy_state':'ready','state':'printing','filename':'Continuous-pointer-test.gcode','progress_pct':65.5,'progress_basis':'display-status','print_duration_s':7200,'total_duration_s':7300,'remaining_s':1800,'eta_at':1900000000,'eta_basis':'progress-average','current_layer':None,'total_layers':None,'filament_used_mm':1234,'fan_pct':40,'ttl_s':90,'age_s':1,'heaters':[{'name':'Nozzle','temp_c':220,'target_c':220,'duty_pct':37.5},{'name':'Bed','temp_c':55,'target_c':55,'duty_pct':12.5}],'temperatures':[{'name':'mcu_temp','temp_c':51.5},{'name':'raspberry_pi','temp_c':53.5}]}}
    printer_path=args.build_dir/'printer_fixture.h'
    fixture(printer,printer_path)
    printer_path.write_text(printer_path.read_text().replace('fixtureFingerprint()', 'printerFixtureFingerprint()').replace('loadFixture(', 'loadPrinterFixture(').replace('loadConfigurationFixture(', 'loadPrinterConfigurationFixture('))
    for suffix in ('o','obj'):
        (args.build_dir/'CMakeFiles/native-gestures.dir'/('gestures.cpp.'+suffix)).unlink(missing_ok=True)
    lvgl=ROOT/'firmware/.pio/libdeps/homelab_s3/lvgl'
    if not (lvgl/'CMakeLists.txt').exists():parser.error('LVGL dependency missing; install the firmware dependencies first')
    commands=[['cmake','-S',str(Path(__file__).parent),'-B',str(args.build_dir),f'-DLVGL_SOURCE_DIR={lvgl}','-DCMAKE_BUILD_TYPE=Release',f'-DHOMELAB_ROTATION={args.rotation}'],['cmake','--build',str(args.build_dir),'--target','native-gestures','-j','8']]
    with (args.build_dir/'gesture-build.log').open('w') as log:
        for command in commands:
            result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
            if result.returncode:print((args.build_dir/'gesture-build.log').read_text()[-8000:],file=sys.stderr);return result.returncode
    result=subprocess.run([str(args.build_dir/'native-gestures'),str(args.output.resolve())],capture_output=True,text=True)
    print(result.stdout,end='');print(result.stderr,end='',file=sys.stderr)
    if ('fixture_sha256='+fingerprint) not in result.stdout:
        print('Gesture fixture fingerprint does not match input',file=sys.stderr)
        return 1
    if f'viewport={width}x{height} rotation={args.rotation}' not in result.stdout:
        print('Gesture viewport does not match selected rotation',file=sys.stderr)
        return 1
    (args.output/'results.txt').write_text(result.stdout+result.stderr)
    for path in args.output.glob('*.ppm'):png(path)
    (args.output/'provenance.json').write_text(json.dumps({'firmware_sha256':hashlib.sha256((ROOT/'firmware/src/main.cpp').read_bytes()).hexdigest(),'rotation':args.rotation,'native_resolution':[width,height],'board_sha256':hashlib.sha256((ROOT/'firmware/src/board.h').read_bytes()).hexdigest(),'model_sha256':hashlib.sha256((ROOT/'firmware/src/model.h').read_bytes()).hexdigest(),'method':'Real LVGL 9.3 pointer state machine with continuous press/move/release samples','preferences':'In-memory simulation; device persistence requires hardware confirmation','passed':result.returncode==0,'passed_checks':sum(line.startswith('PASS:') for line in result.stdout.splitlines()),'management_operations':'In-memory simulation; no collector or Wi-Fi changes'},indent=2)+'\n')
    return result.returncode

if __name__=='__main__':raise SystemExit(main())
