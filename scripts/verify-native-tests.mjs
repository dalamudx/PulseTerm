// Verify the exact Rust test scope before a runner can report native component success.
export function verifyGroup(stdout, expected) {
  const executed = [...stdout.matchAll(/^test (\S+) \.\.\. (ok|FAILED|ignored)\r?$/gm)];
  const summaries = [...stdout.matchAll(/^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;/gm)];
  if (!expected.length || new Set(expected).size !== expected.length
      || summaries.length !== 1 || summaries[0][1] !== 'ok'
      || [...stdout.matchAll(/^test result:/gm)].length !== 1
      || Number(summaries[0][2]) !== expected.length
      || summaries[0].slice(3, 6).some(n => Number(n) !== 0)) {
    throw new Error('Incomplete native test summary');
  }
  const names = executed.map(match => match[1]);
  if (names.length !== expected.length || new Set(names).size !== names.length
      || expected.some(name => !names.includes(name)) || executed.some(match => match[2] !== 'ok')) {
    throw new Error('Native test identities or individual outcomes differ from the source manifest');
  }
  return names;
}

export function nativeTestsPassed(result, expected) {
  if (!expected || result.error || result.signal !== null || result.status !== 0) return false;
  try {
    verifyGroup(result.stdout ?? '', expected);
    return true;
  } catch {
    return false;
  }
}
