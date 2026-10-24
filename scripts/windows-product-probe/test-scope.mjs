import fs from 'node:fs';
import path from 'node:path';

export const groups = [
  { binary: 'xtask-tests.exe', target: 'xtask', file: 'apps/desktop/src-tauri/src/settings_file_windows_native_tests.rs', prefix: 'windows_settings::native_tests::', count: 7 },
  { binary: 'xtask-tests.exe', target: 'xtask', file: 'apps/desktop/src-tauri/src/product_app_data_windows_tests.rs', prefix: 'workspace_app_data::windows_tests::', count: 2 },
  { binary: 'xtask-tests.exe', target: 'xtask', file: 'apps/desktop/src-tauri/src/database_windows_tests.rs', prefix: 'workspace_database::tests::', count: 5 },
  { binary: 'xtask-tests.exe', target: 'xtask', file: 'apps/desktop/src-tauri/src/keymap_file_windows_tests.rs', prefix: 'windows_keymap::tests::', count: 4 },
  { binary: 'pty-tests.exe', target: 'pulse_pty', file: 'crates/pulse-pty/src/profiles_windows.rs', prefix: 'profiles::windows::tests::', count: 5 },
];
export function definitions(root) {
  return groups.flatMap(group => {
    const text = fs.readFileSync(path.join(root, group.file), 'utf8');
    const names = [...text.matchAll(/#\[(?:tokio::)?test(?:\([^\]]*\))?\]\s*(?:async\s+)?fn\s+(\w+)\s*\(/g)].map(match => group.prefix + match[1]);
    if (names.length !== group.count || new Set(names).size !== names.length) throw new Error(`Review native coverage change: ${group.file}`);
    return names.map(name => ({ binary: group.binary, target: group.target, name }));
  });
}
