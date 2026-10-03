import type { Terminal } from "@xterm/headless";

/** Install before writing any bytes. Rust is the sole protocol-query responder.
 * Shared headless/DOM probe policy; color setters still fall through.
 */
export function suppressQueryReplies(
  terminal: Pick<Terminal, "parser">,
): () => void {
  const registrations = [
    terminal.parser.registerCsiHandler({ final: "c" }, () => true),
    terminal.parser.registerCsiHandler({ prefix: ">", final: "c" }, () => true),
    terminal.parser.registerCsiHandler({ final: "n" }, () => true),
    terminal.parser.registerCsiHandler({ prefix: "?", final: "n" }, () => true),
    terminal.parser.registerCsiHandler(
      { intermediates: "$", final: "p" },
      () => true,
    ),
    terminal.parser.registerCsiHandler(
      { prefix: "?", intermediates: "$", final: "p" },
      () => true,
    ),
    terminal.parser.registerCsiHandler({ prefix: ">", final: "q" }, () => true),
    terminal.parser.registerCsiHandler({ final: "t" }, (params) =>
      [14, 16, 18, 19, 20, 21].includes(Number(params[0])),
    ),
    terminal.parser.registerDcsHandler(
      { intermediates: "$", final: "q" },
      () => true,
    ),
    terminal.parser.registerOscHandler(4, (data) =>
      data.split(";").includes("?"),
    ),
    ...[10, 11, 12].map((code) =>
      terminal.parser.registerOscHandler(code, (data) =>
        data.split(";").includes("?"),
      ),
    ),
    // Never let remote content read or write the browser clipboard.
    terminal.parser.registerOscHandler(52, () => true),
  ];
  return () => {
    for (const registration of registrations) registration.dispose();
  };
}
