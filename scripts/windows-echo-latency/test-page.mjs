import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const html = readFileSync(new URL('./echo.html', import.meta.url), 'utf8');
const source = html.slice(html.lastIndexOf('<script>') + 8, html.lastIndexOf('</script>'));
for (const renderer of ['dom','webgl']) for (const diagnostic of [false,true]) {
  const messages = []; let terminal, lost;
  class Terminal {
    constructor() { terminal=this; this.textarea={}; this.buffer={active:{cursorX:0,cursorY:0}}; this.callbacks=0; }
    open() {} focus() {} loadAddon() {}
    onData(fn) {this.data=fn;} onRender(fn) {this.render=fn;}
    write(data,callback) {if(data.includes('255;0;0'))this.buffer.active.cursorX+=2;if(callback){this.callbacks++;callback();}}
  }
  const document={title:'',activeElement:null,hasFocus:()=>true,getElementById:()=>({})};
  const context={URLSearchParams,Terminal,WebglAddon:{WebglAddon:class{onContextLoss(fn){lost=fn;}}},document,
    location:{search:`?renderer=${renderer}${diagnostic?'&diagnostic=1':''}`},
    window:{chrome:{webview:{postMessage:value=>messages.push(value)}},addEventListener(){}}};
  vm.runInNewContext(source,context);document.activeElement=terminal.textarea;terminal.data('2');
  for(let i=0;i<110;i++){terminal.data('1');if(diagnostic)terminal.render();}
  assert.equal(terminal.buffer.active.cursorX,220);
  assert.equal(terminal.callbacks,diagnostic?110:0);
  if(!diagnostic)assert.deepEqual(messages,[]);
  else{
    for(const type of ['input','write','render'])assert.equal(messages.filter(x=>x.startsWith(`echo-event ${type} `)).length,110);
    assert(messages.every(x=>/^echo-event [a-z-]+ (\d+ ){4}\d+$/.test(x)));
    if(renderer==='webgl'){lost();assert.equal(document.title,'echo-webgl-lost');assert(messages.at(-1).startsWith('echo-event context-lost '));}
  }
}
console.log('page diagnostic tests passed: silent normal mode, 110 input/write/render counts, focus and context loss (Terminal stub only)');
