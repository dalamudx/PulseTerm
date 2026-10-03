// xterm's public documentOverride isolates trusted style creation. Never patch
// window.document or grant nonces to remote content; no HTML/URL handlers exist here.
export function nonceDocument(document: Document): Document {
  const nonce = document.querySelector<HTMLStyleElement>(
    "#terminal-style-nonce",
  )?.nonce;
  if (!nonce || nonce.includes("__TAURI_"))
    throw new Error("terminal.style_nonce_missing");
  return new Proxy(document, {
    get(target, key) {
      if (key === "createElement")
        return (tag: string, options?: ElementCreationOptions) => {
          const element = target.createElement(tag, options);
          if (tag.toLowerCase() === "style")
            element.setAttribute("nonce", nonce);
          return element;
        };
      const value = Reflect.get(target, key, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
}
