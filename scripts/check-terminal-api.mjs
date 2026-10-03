// Check the actual default-feature downstream API, not a workspace-unified build.
import { mkdtempSync, writeFileSync, rmSync, mkdirSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const dir = mkdtempSync(join(tmpdir(), 'pulseterm-api-'));
try {
  mkdirSync(join(dir, 'src'));
  const dependency = fileURLToPath(new URL('../crates/pulse-term', import.meta.url)).replaceAll('\\', '/');
  writeFileSync(join(dir, 'Cargo.toml'), `[package]\nname="terminal-api-consumer"\nversion="0.0.0"\nedition="2024"\n[dependencies]\npulse-term={path=${JSON.stringify(dependency)},default-features=false}\n[patch.crates-io]\nalacritty_terminal={path=${JSON.stringify(fileURLToPath(new URL('../vendor/alacritty_terminal',import.meta.url)).replaceAll('\\','/'))}}\n`);
  writeFileSync(join(dir, 'Cargo.lock'), readFileSync(new URL('../Cargo.lock', import.meta.url)));
  const env = { ...process.env, CARGO_TARGET_DIR: fileURLToPath(new URL('../target/terminal-api-check', import.meta.url)) };
  function check(source) {
    writeFileSync(join(dir, 'src/main.rs'), source);
    const result = spawnSync('cargo', ['check', '--manifest-path', join(dir,'Cargo.toml'), '--offline'], { env, encoding:'utf8', timeout:120000 });
    if(result.error) throw result.error;
    return result;
  }
  const valid=check('fn main() { let _ = std::mem::size_of::<pulse_term::TerminalProbe>(); }');
  if(valid.status!==0) throw new Error(valid.stderr);
  for (const [source, expected] of [
    ['fn main() { let _: fn(&pulse_term::TerminalProbe) -> String = pulse_term::TerminalProbe::screen_replay; }', 'screen_replay'],
    ['fn main() { let _ = std::mem::size_of::<pulse_term::session::EchoTransport>(); }', 'EchoTransport'],
  ]) {
    const result=check(source);
    if(result.status===0 || !result.stderr.includes(expected)) throw new Error(`Forbidden production API remains available: ${expected}\n${result.stderr}`);
  }
  console.log('Default-feature consumer compiles; legacy screen replay and EchoTransport are unavailable.');
} finally { rmSync(dir,{recursive:true,force:true}); }
