export interface DisplayPreferences {
  readonly fontSize: number;
  readonly lineHeightPercent?: number;
  readonly letterSpacing?: number;
  readonly fontFamily: string;
  readonly fontFallback?: string;
  readonly fontLigatures?: boolean;
  readonly cursorBlink: boolean;
  readonly scrollOnInput?: boolean;
  readonly scrollOnOutput?: boolean;
  readonly macOptionIsMeta?: boolean;
  readonly backspaceSend?: "del" | "bs";
  readonly deleteSend?: "csi" | "del";
  readonly copyOnSelect?: boolean;
  readonly rightClickPaste?: boolean;
  readonly confirmClose?: boolean;
}
/** Validated preferences never grant view/input authorization. */
export function displayPreferences(
  input: unknown,
): Readonly<DisplayPreferences> {
  if (!input || typeof input !== "object" || Array.isArray(input))
    throw Error("terminal.display_invalid");
  const r = input as Record<string, unknown>;
  if (
    Object.keys(r).some(
      (key) =>
        ![
          "fontSize",
          "lineHeightPercent",
          "letterSpacing",
          "fontFamily",
          "fontFallback",
          "fontLigatures",
          "cursorBlink",
          "scrollOnInput",
          "scrollOnOutput",
          "macOptionIsMeta",
          "backspaceSend",
          "deleteSend",
          "copyOnSelect",
          "rightClickPaste",
          "confirmClose",
        ].includes(key),
    ) ||
    ["fontSize", "fontFamily", "cursorBlink"].some(
      (key) => !Object.hasOwn(r, key),
    ) ||
    typeof r.fontSize !== "number" ||
    !Number.isInteger(r.fontSize) ||
    r.fontSize < 6 ||
    r.fontSize > 72 ||
    (Object.hasOwn(r, "lineHeightPercent") &&
      (typeof r.lineHeightPercent !== "number" ||
        !Number.isInteger(r.lineHeightPercent) ||
        r.lineHeightPercent < 100 ||
        r.lineHeightPercent > 300)) ||
    (Object.hasOwn(r, "letterSpacing") &&
      (typeof r.letterSpacing !== "number" ||
        !Number.isInteger(r.letterSpacing) ||
        r.letterSpacing < 0 ||
        r.letterSpacing > 8)) ||
    typeof r.fontFamily !== "string" ||
    r.fontFamily.length > 256 ||
    Array.from(r.fontFamily).length > 128 ||
    !/^[\p{L}\p{N}_-](?:[\p{L}\p{N} _-]*[\p{L}\p{N}_-])?$/u.test(
      r.fontFamily,
    ) ||
    (Object.hasOwn(r, "fontFallback") &&
      (typeof r.fontFallback !== "string" ||
        Array.from(r.fontFallback).length > 128 ||
        !/^[\p{L}\p{N}_-](?:[\p{L}\p{N} _-]*[\p{L}\p{N}_-])?$/u.test(
          r.fontFallback,
        ))) ||
    (Object.hasOwn(r, "fontLigatures") &&
      typeof r.fontLigatures !== "boolean") ||
    typeof r.cursorBlink !== "boolean" ||
    (Object.hasOwn(r, "scrollOnInput") &&
      typeof r.scrollOnInput !== "boolean") ||
    (Object.hasOwn(r, "scrollOnOutput") &&
      typeof r.scrollOnOutput !== "boolean") ||
    (Object.hasOwn(r, "macOptionIsMeta") &&
      typeof r.macOptionIsMeta !== "boolean") ||
    (Object.hasOwn(r, "backspaceSend") &&
      r.backspaceSend !== "del" &&
      r.backspaceSend !== "bs") ||
    (Object.hasOwn(r, "deleteSend") &&
      r.deleteSend !== "csi" &&
      r.deleteSend !== "del") ||
    (Object.hasOwn(r, "copyOnSelect") && typeof r.copyOnSelect !== "boolean") ||
    (Object.hasOwn(r, "rightClickPaste") &&
      typeof r.rightClickPaste !== "boolean") ||
    (Object.hasOwn(r, "confirmClose") && typeof r.confirmClose !== "boolean")
  )
    throw Error("terminal.display_invalid");
  return Object.freeze({
    fontSize: r.fontSize,
    ...(Object.hasOwn(r, "lineHeightPercent")
      ? { lineHeightPercent: r.lineHeightPercent as number }
      : {}),
    ...(Object.hasOwn(r, "letterSpacing")
      ? { letterSpacing: r.letterSpacing as number }
      : {}),
    fontFamily: r.fontFamily,
    ...(Object.hasOwn(r, "fontFallback")
      ? { fontFallback: r.fontFallback as string }
      : {}),
    ...(Object.hasOwn(r, "fontLigatures")
      ? { fontLigatures: r.fontLigatures as boolean }
      : {}),
    cursorBlink: r.cursorBlink,
    ...(Object.hasOwn(r, "scrollOnInput")
      ? { scrollOnInput: r.scrollOnInput as boolean }
      : {}),
    ...(Object.hasOwn(r, "scrollOnOutput")
      ? { scrollOnOutput: r.scrollOnOutput as boolean }
      : {}),
    ...(Object.hasOwn(r, "macOptionIsMeta")
      ? { macOptionIsMeta: r.macOptionIsMeta as boolean }
      : {}),
    ...(Object.hasOwn(r, "backspaceSend")
      ? { backspaceSend: r.backspaceSend as "del" | "bs" }
      : {}),
    ...(Object.hasOwn(r, "deleteSend")
      ? { deleteSend: r.deleteSend as "csi" | "del" }
      : {}),
    ...(Object.hasOwn(r, "copyOnSelect")
      ? { copyOnSelect: r.copyOnSelect as boolean }
      : {}),
    ...(Object.hasOwn(r, "rightClickPaste")
      ? { rightClickPaste: r.rightClickPaste as boolean }
      : {}),
    ...(Object.hasOwn(r, "confirmClose")
      ? { confirmClose: r.confirmClose as boolean }
      : {}),
  });
}
