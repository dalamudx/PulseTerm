// Static application copy only. User, terminal and server data are deliberately outside this check.
import { existsSync, readFileSync } from "node:fs";
import { dirname, extname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { englishMessages } from "../src/workspace-language";

const root = fileURLToPath(new URL("../src/", import.meta.url));
const pending = [resolve(root, "workspace-main.ts")];
const seen = new Set<string>();
const han = /\p{Script=Han}/u;
const messageArguments = new Map([
  ["message", 0],
  ["uiText", 1],
  ["uiNode", 1],
  ["uiAttribute", 2],
  ["translate", 1],
]);
const textProperties = new Set([
  "textContent",
  "innerText",
  "title",
  "ariaLabel",
  "placeholder",
]);
const textAttributes = new Set(["aria-label", "title", "placeholder"]);
const failures: string[] = [];
let checked = 0;

function text(node: ts.Node | undefined): string | undefined {
  return node &&
    (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node))
    ? node.text
    : undefined;
}
function fragments(node: ts.Node | undefined): string[] {
  if (!node) return [];
  const value = text(node);
  if (value !== undefined) return han.test(value) ? [value] : [];
  if (ts.isTemplateExpression(node))
    return [
      node.head.text,
      ...node.templateSpans.map((span) => span.literal.text),
    ].filter((value) => han.test(value));
  if (ts.isConditionalExpression(node))
    return [...fragments(node.whenTrue), ...fragments(node.whenFalse)];
  if (
    ts.isBinaryExpression(node) &&
    node.operatorToken.kind === ts.SyntaxKind.PlusToken
  )
    return [...fragments(node.left), ...fragments(node.right)];
  return [];
}
function dependency(file: string, specifier: string): void {
  if (!specifier.startsWith(".")) return;
  const base = resolve(dirname(file), specifier);
  if (extname(base) && ![".ts", ".tsx"].includes(extname(base))) return;
  const found = [
    base,
    `${base}.ts`,
    `${base}.tsx`,
    resolve(base, "index.ts"),
  ].find((p) => existsSync(p) && /\.tsx?$/.test(p));
  if (!found || relative(root, found).startsWith(".."))
    throw Error(
      `Unresolved workspace dependency: ${relative(root, file)} -> ${specifier}`,
    );
  if (!seen.has(found)) pending.push(found);
}
while (pending.length) {
  const candidate = pending.pop();
  if (!candidate || seen.has(candidate)) continue;
  const file = candidate;
  seen.add(file);
  const source = ts.createSourceFile(
    file,
    readFileSync(file, "utf8"),
    ts.ScriptTarget.Latest,
    true,
  );
  const bindings = new Map<string, number>();
  for (const statement of source.statements) {
    if (
      ts.isImportDeclaration(statement) ||
      ts.isExportDeclaration(statement)
    ) {
      const specifier = text(statement.moduleSpecifier);
      if (specifier) dependency(file, specifier);
      if (
        ts.isImportDeclaration(statement) &&
        specifier?.endsWith("workspace-language")
      ) {
        const imports = statement.importClause?.namedBindings;
        if (imports && ts.isNamedImports(imports)) {
          for (const item of imports.elements) {
            const argument = messageArguments.get(
              item.propertyName?.text ?? item.name.text,
            );
            if (argument !== undefined) bindings.set(item.name.text, argument);
          }
        }
      }
    }
  }
  function report(node: ts.Node, reason: string, value: string): void {
    const { line } = source.getLineAndCharacterOfPosition(
      node.getStart(source),
    );
    failures.push(
      `${relative(root, file)}:${line + 1}: ${reason}: ${JSON.stringify(value.slice(0, 140))}`,
    );
  }
  function visit(node: ts.Node): void {
    if (ts.isCallExpression(node)) {
      if (ts.isIdentifier(node.expression)) {
        const argument = bindings.get(node.expression.text);
        const value =
          argument === undefined ? undefined : text(node.arguments[argument]);
        if (value && han.test(value)) {
          checked++;
          if (!Object.hasOwn(englishMessages, value))
            report(node, "Missing English catalog entry", value);
        }
        const argumentNode =
          argument === undefined ? undefined : node.arguments[argument];
        if (
          argumentNode &&
          value === undefined &&
          ts.isTemplateExpression(argumentNode)
        ) {
          for (const part of fragments(argumentNode))
            report(
              node,
              "Use a catalog message with literal interpolation arguments",
              part,
            );
        }
      }
      if (ts.isPropertyAccessExpression(node.expression)) {
        const name = node.expression.name.text;
        const attribute = text(node.arguments[0]);
        const value =
          name === "setAttribute" && attribute && textAttributes.has(attribute)
            ? node.arguments[1]
            : name === "createTextNode"
              ? node.arguments[0]
              : undefined;
        for (const part of fragments(value))
          report(node, "Unbound application text", part);
      }
      if (node.expression.kind === ts.SyntaxKind.ImportKeyword) {
        const specifier = text(node.arguments[0]);
        if (specifier) dependency(file, specifier);
      }
    }
    if (
      ts.isBinaryExpression(node) &&
      node.operatorToken.kind === ts.SyntaxKind.EqualsToken &&
      ts.isPropertyAccessExpression(node.left) &&
      textProperties.has(node.left.name.text)
    ) {
      for (const value of fragments(node.right))
        report(node, "Unbound application property", value);
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
}
if (failures.length) {
  console.error(failures.join("\n"));
  process.exitCode = 1;
} else {
  console.log(
    `Workspace static copy: ${seen.size} source modules, ${checked} literal messages, no missing translations or unbound Chinese text.`,
  );
}
