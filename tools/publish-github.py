#!/usr/bin/env python3
"""Publish local Noxel builds with the existing Ed25519 signing key."""
import hashlib,json,pathlib,re,subprocess,sys,tempfile
ROOT=pathlib.Path(__file__).resolve().parents[1]
def run(*args,**kwargs):return subprocess.run(args,cwd=ROOT,check=True,**kwargs)
def main():
 version=sys.argv[1] if len(sys.argv)>1 else ''
 if not re.fullmatch(r'\d+\.\d+\.\d+',version):raise SystemExit('Usage: tools/publish-github.py X.Y.Z')
 key=ROOT/'.keys/release.pem';binary=ROOT/'готовый продукт/Noxel';setup=ROOT/'готовый продукт/noxel-setup-linux-x86_64'
 for f in [key,binary,setup]:
  if not f.is_file():raise SystemExit('Missing '+str(f.relative_to(ROOT)))
 tag='v'+version;repo='magmar1s/Noxel';name='noxel-linux-x86_64-'+version
 signature=run('openssl','pkeyutl','-sign','-rawin','-inkey',str(key),'-in',str(binary),capture_output=True).stdout.hex()
 manifest={'version':version,'notes':['Java, Minecraft и Horizon в установщике. Выпуски через GitHub.'],'news':[],'files':{'linux-x86_64':{'version':version,'url':f'https://github.com/{repo}/releases/download/{tag}/{name}','sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'sig':signature,'size':binary.stat().st_size}}}
 # A draft stays private until the signed manifest and assets are ready.
 exists=subprocess.run(['gh','release','view',tag,'--repo',repo],cwd=ROOT,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0
 if not exists:
  run('gh','release','create',tag,'--repo',repo,'--target','main','--draft','--title','Noxel '+version,'--notes','Автор: magmar1s. Linux x86_64. Установщик: Java, Minecraft, Horizon. Лицензия MIT.')
 with tempfile.TemporaryDirectory(prefix='noxel-release-') as t:
  p=pathlib.Path(t)/name;p.write_bytes(binary.read_bytes())
  checksum=pathlib.Path(t)/'SHA256SUMS';files=[p,setup,ROOT/'готовый продукт/Noxel-linux-x86_64.tar.gz'];checksum.write_text(''.join(hashlib.sha256(f.read_bytes()).hexdigest()+'  '+f.name+'\n' for f in files))
  (ROOT/'distribution/launcher.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
  run('gh','release','upload',tag,*map(str,files),str(checksum),'--repo',repo,'--clobber')
 run('git','add','distribution/launcher.json');run('git','commit','-m','Publish signed manifest for '+version);run('git','push','origin','main')
 run('gh','release','edit',tag,'--repo',repo,'--draft=false','--latest')
 print(f'https://github.com/{repo}/releases/tag/{tag}')
if __name__=='__main__':main()
