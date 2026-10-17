// SDK joiner offsets are UTF-16 positions, not terminal columns. The SDK maps
// them with our shared width provider; never join wide/combining characters.
const operators = /<=>|!==|===|=>|->|!=|==|<=|>=|<<|>>|&&|\|\||::|:=|\.\.\./g;
export function operatorLigatures(text: string): [number, number][] {
  const ranges: [number, number][] = [];
  operators.lastIndex = 0;
  for (
    let match = operators.exec(text);
    match && ranges.length < 64;
    match = operators.exec(text)
  ) {
    ranges.push([match.index, match.index + match[0].length]);
  }
  return ranges;
}
