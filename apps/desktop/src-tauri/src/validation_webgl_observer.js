// Validation-only: public canvas state. No commands, SDK internals, or execution authority.
(() => {
  const node = document.createElement("button");
  node.type = "button";
  node.disabled = true;
  node.id = 'pulseterm-renderer-probe';
  node.className = "renderer-validation-probe";
  node.textContent = "Renderer probe";
  node.setAttribute('aria-label', 'Renderer probe');
  document.body.append(node);
  let shapeDifference = -1;
  document.fonts.load('18px "JetBrains Mono"').then(() => {
    const canvas = document.createElement('canvas');
    canvas.width = 80; canvas.height = 40;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.font = '18px "JetBrains Mono"';
    ctx.fillStyle = '#fff'; ctx.textBaseline = 'top';
    const width = ctx.measureText('!').width;
    ctx.fillText('!=', 0, 0);
    const together = ctx.getImageData(0, 0, 80, 40).data;
    ctx.clearRect(0, 0, 80, 40);
    ctx.fillText('!', 0, 0); ctx.fillText('=', width, 0);
    const separate = ctx.getImageData(0, 0, 80, 40).data;
    shapeDifference = 0;
    for (let i = 3; i < together.length; i += 4) {
      if (together[i] !== separate[i]) shapeDifference++;
    }
  });
  const deadline = performance.now() + 120000;
  const timer = setInterval(() => {
    if (performance.now() > deadline) { clearInterval(timer); return; }
    const canvases = Array.from(document.querySelectorAll('.xterm-screen canvas'));
    const active = canvases.filter(canvas => {
      try { const gl = canvas.getContext('webgl2'); return gl && !gl.isContextLost(); }
      catch { return false; }
    }).length;
    const text = `renderer-probe:${active > 0 ? 'webgl2' : 'dom'};canvases=${canvases.length}`;
    node.title = `${text};shape-difference=${shapeDifference}`;
    node.setAttribute("aria-description", node.title);
  }, 50);
  addEventListener('pagehide', () => clearInterval(timer), { once: true });
})();
