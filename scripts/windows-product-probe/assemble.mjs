// Conservative compiled scope. Missing or unverified original license text blocks the ZIP.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { copyLockedPackage } from '../windows-storage-probe/assemble.mjs';
import { definitions } from './test-scope.mjs';
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
export function assemble(root, artifactsDirectory, kit, compilerDocs) {
  const artifacts = ['xtask-tests','core-tests','xtask-main','workspace-main'].flatMap(name => fs.readFileSync(path.join(artifactsDirectory, name+'.jsonl'),'utf8').split('\n').filter(Boolean).map(JSON.parse)).filter(a => a.reason === 'compiler-artifact');
  const tests = definitions(root);
  if (tests.length !== 22) throw new Error('Review selected native coverage');
  fs.mkdirSync(kit,{recursive:true});
  for (const [target, test, name] of [['xtask',true,'xtask-tests.exe'],['pulse_pty',true,'pty-tests.exe'],['xtask',false,'backend-probe.exe'],['pulseterm-workspace',false,'PulseTerm-Workspace.exe']]) {
    const candidates = [...new Set(artifacts.filter(a => a.target.name === target && a.profile.test === test && a.executable?.endsWith('.exe')).map(a=>a.executable))];
    if (candidates.length !== 1) throw new Error(`Missing or ambiguous program: ${target}/${test}`);
    const bytes=fs.readFileSync(candidates[0]);
    if (bytes[0] !== 0x4d || bytes[1] !== 0x5a) throw new Error(`Not a PE program: ${target}`);
    for (const item of tests.filter(t=>t.binary===name)) if (!bytes.includes(Buffer.from(item.name))) throw new Error(`Test registration absent from PE: ${item.name}`);
    fs.copyFileSync(candidates[0],path.join(kit,name));
  }
  fs.writeFileSync(path.join(kit,'native-tests.json'),JSON.stringify({schemaVersion:1,target:'x86_64-pc-windows-gnu',tests:tests.map(({binary,name})=>({binary,name}))},null,2)+'\n');
  const legal=path.join(kit,'third-party'); fs.mkdirSync(legal,{recursive:true});
  const cargoHome=process.env.CARGO_HOME||path.join(process.env.HOME,'.cargo');
  const lock=fs.readFileSync(path.join(root,'Cargo.lock'),'utf8');
  const packages=[...new Map(artifacts.filter(a=>a.package_id.startsWith('registry+')).map(a=>[a.package_id,a])).values()].map(a=>copyLockedPackage(a,lock,cargoHome,legal));
  const rustRoot=execFileSync('rustc',['--print','sysroot'],{encoding:'utf8'}).trim();
  const rustDocs=path.join(rustRoot,'share/doc/rust');
  for (const file of ['COPYRIGHT.html','COPYRIGHT-library.html']) fs.copyFileSync(path.join(rustDocs,file),path.join(legal,'Rust-'+file));
  fs.cpSync(path.join(rustDocs,'licenses'),path.join(legal,'rust-licenses'),{recursive:true});
  for (const [source,target] of [['gcc-mingw-w64-base/copyright','NOTICE-GCC-Runtime.txt'],['mingw-w64-common/copyright','NOTICE-MinGW.txt']]) fs.copyFileSync(path.join(compilerDocs,source),path.join(legal,target));
  fs.copyFileSync('/usr/share/common-licenses/GPL-3',path.join(legal,'GPL-3.txt'));
  fs.copyFileSync(path.join(root,'LICENSE-MIT'),path.join(kit,'LICENSE-PulseTerm-MIT.txt'));
  for (const file of ['run.ps1','native-process.ps1','README.md']) fs.copyFileSync(path.join(root,'scripts/windows-product-probe',file),path.join(kit,file));
  const sourceHashes={};
  for (const relative of ['Cargo.toml','Cargo.lock','.mise.toml','xtask/Cargo.toml','xtask/src/main.rs',...fs.readdirSync(path.join(root,'crates'),{recursive:true}).filter(p=>p.endsWith('.rs')||p.endsWith('Cargo.toml')).map(p=>'crates/'+p),...fs.readdirSync(path.join(root,'apps/desktop/src-tauri/src'),{recursive:true}).filter(p=>p.endsWith('.rs')).map(p=>'apps/desktop/src-tauri/src/'+p),...fs.readdirSync(path.join(root,'ui/dist'),{recursive:true}).filter(p=>fs.statSync(path.join(root,'ui/dist',p)).isFile()).map(p=>'ui/dist/'+p),...fs.readdirSync(path.join(root,'scripts/windows-product-probe')).filter(p=>/\.(mjs|ps1|md)$/.test(p)).map(p=>'scripts/windows-product-probe/'+p)]) sourceHashes[relative]=sha(fs.readFileSync(path.join(root,relative)));
  const build={schemaVersion:1,runnerVersion:1,target:'x86_64-pc-windows-gnu',profile:'dev; static GNU runtime; debug info disabled',rustc:execFileSync('rustc',['--version'],{encoding:'utf8'}).trim(),revision:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim(),dirty:execFileSync('git',['status','--porcelain','--untracked-files=no'],{cwd:root,encoding:'utf8'}).trim().length>0,productionStartupValidated:false,nativeWindowsExecution:'pending',msvcBuildValidated:false,latencyComparisonValid:false,sourceHashes,thirdPartyScope:'Conservative compiler-artifact scope including build-time crates; not exact shipped dependency/system-DLL attribution or release approval',packages};
  fs.writeFileSync(path.join(kit,'build-info.json'),JSON.stringify(build,null,2)+'\n');
  const files=fs.readdirSync(kit,{recursive:true}).filter(p=>p!=='SHA256SUMS'&&fs.statSync(path.join(kit,p)).isFile()).sort();
  fs.writeFileSync(path.join(kit,'SHA256SUMS'),files.map(file=>sha(fs.readFileSync(path.join(kit,file)))+'  '+file).join('\n')+'\n');
  return {nativeCases:tests.length,lockedPackages:packages.length,files:files.length};
}
if (process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)) console.log(JSON.stringify(assemble(...process.argv.slice(2))));
