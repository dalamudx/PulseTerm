import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const parent=process.env.TMPDIR||process.env.TEMP;
if(!parent)throw new Error('Set a task-owned TMPDIR/TEMP first');
const root=fs.mkdtempSync(path.join(parent,'product-process-test-'));
const directory=path.dirname(fileURLToPath(import.meta.url));
try {
  const source=path.join(root,'fast-exit.rs'),exe=path.join(root,os.platform()==='win32'?'fast-exit.exe':'fast-exit');
  fs.writeFileSync(source,'fn main(){println!("fixture stdout");eprintln!("fixture stderr \\u{4e2d}");if std::env::args().any(|a|a=="--fail"){std::process::exit(101)}}\n');
  const build=spawnSync('rustc',['--edition=2024',source,'-o',exe],{encoding:'utf8',timeout:120000});
  if(build.error||build.status!==0)throw new Error(build.stderr||String(build.error));
  const engine=process.env.PULSETERM_TEST_PWSH||(process.platform==='win32'?'powershell':'pwsh');
  const test=spawnSync(engine,['-NoProfile','-NonInteractive','-File',path.join(directory,'test-process.ps1'),'-TestRoot',path.join(root,'logs with spaces'),'-FixtureExe',exe],{encoding:'utf8',timeout:120000});
  process.stdout.write(test.stdout||'');process.stderr.write(test.stderr||'');
  if(test.error||test.status!==0)throw new Error(test.stderr||String(test.error));
} finally {fs.rmSync(root,{recursive:true,force:true});}
