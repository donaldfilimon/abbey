import datetime,hashlib,json,os,pathlib,subprocess,sys,time
root=pathlib.Path.cwd(); label=sys.argv[1]; argv=sys.argv[2:]
out=root/'docs/verification/2026-10-25-updater-zip9'; out.mkdir(exist_ok=True)
env=os.environ.copy(); env.update(CARGO_TARGET_DIR=str(root/'.work/zip9/target'), CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0', ABI_WDBX_PATH=':memory:'); env.pop('ABI_WDBX_PERSIST',None)
env.setdefault('ABBEY_ZIP_OPENSSL',__import__('shutil').which('openssl') or 'openssl')
selected={k:env.get(k) for k in ('CARGO_TARGET_DIR','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG','CARGO_PROFILE_TEST_DEBUG','ABI_WDBX_PATH','ABI_WDBX_PERSIST','ABBEY_DEP_SCAN_REQUIRE','ABBEY_DESKTOP_UPDATE_ARTIFACT','ABBEY_ZIP_OPENSSL')}
def digest():
 paths=subprocess.check_output(['git','ls-files','-co','--exclude-standard','Cargo.toml','Cargo.lock','rust-toolchain.toml','build.rs','.cargo','src','tools','desktop/src-tauri','desktop/scripts/qualify-updater-windows.ps1'],text=True).splitlines()
 return {p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest() for p in sorted(set(paths)) if pathlib.Path(p).is_file()}
siblings={p:{'head':subprocess.check_output(['git','-C',p,'rev-parse','HEAD'],text=True).strip(),'status':subprocess.check_output(['git','-C',p,'status','--short'],text=True)} for p in ('../abi','../wdbx')}
r={'siblings':siblings,'command':argv,'environment':selected,'stdin':'DEVNULL','start':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_before':digest(),'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'available_bytes_before':__import__('shutil').disk_usage(root).free}
with (out/(label+'.stdout')).open('w') as stdout,(out/(label+'.stderr')).open('w') as stderr:
 try:r['exit']=subprocess.run(argv,env=env,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr).returncode
 except Exception as e:r['exit']=127;r['error']=str(e)
r['outputs']={p.name:{'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'bytes':p.stat().st_size} for p in (out/(label+'.stdout'),out/(label+'.stderr'))}
r.update(end=datetime.datetime.now(datetime.timezone.utc).isoformat(),source_after=digest(),available_bytes_after=__import__('shutil').disk_usage(root).free)
(out/(label+'.json')).write_text(json.dumps(r,indent=2)+'\n');print(label,r['exit'],'free GiB',round(r['available_bytes_after']/1024**3,2));sys.exit(r['exit'])
