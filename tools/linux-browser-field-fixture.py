#!/usr/bin/env python3
"""Local Firefox acceptance page. Reports field identity/geometry, not values."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sys

state, command, ready = map(Path, sys.argv[1:4])
page = """<!doctype html><html lang="en"><meta charset="utf-8">
<title>OKBS browser acceptance</title>
<style>body {margin:40px;font:20px sans-serif} input, textarea, #editable {display:block;width:500px;margin:16px 0;padding:8px;font:20px monospace;border:2px solid #777} #editable {min-height:70px}</style>
<label for="first">First synthetic field</label><input id="first" value="synthetic first field">
<label for="password">Synthetic password</label><input id="password" type="password" value="synthetic password">
<label for="second">Second synthetic field</label><textarea id="second">synthetic second field</textarea>
<div id="editable" contenteditable="true" role="textbox" aria-label="Synthetic editable document">synthetic editable document</div>
<script>
function focusField(id, offset=4) {
 const field=document.getElementById(id); field.focus();
 if (id==='editable') { const range=document.createRange();range.setStart(field.firstChild,offset);range.collapse(true);const selection=getSelection();selection.removeAllRanges();selection.addRange(range); }
 else field.setSelectionRange(offset,offset);
}
let busy=false;
setInterval(async()=>{
 if(busy) return;busy=true;
 try { const action=await (await fetch('/command',{cache:'no-store'})).json();
  if(action.field) focusField(action.field,action.offset??4);
  const field=document.activeElement;const rect=field.getBoundingClientRect();
  await fetch('/state',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({focus:field.id,documentFocus:document.hasFocus(),visibility:document.visibilityState,rect:[rect.x,rect.y,rect.width,rect.height]})});
 } finally {busy=false;}
},50);
addEventListener('load',()=>focusField('first'));
</script></html>""".encode()

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def do_GET(self):
        if self.path == "/command":
            value = {}
            if command.exists():
                value=json.loads(command.read_text());command.unlink()
            data=json.dumps(value).encode();kind="application/json"
        else: data=page;kind="text/html; charset=utf-8"
        self.send_response(200);self.send_header("Content-Type",kind)
        self.send_header("Content-Length",str(len(data)));self.end_headers();self.wfile.write(data)
    def do_POST(self):
        data=json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        temporary=state.with_suffix(".new");temporary.write_text(json.dumps(data));temporary.replace(state)
        self.send_response(204);self.end_headers()

server=ThreadingHTTPServer(("127.0.0.1",0),Handler)
ready.write_text(str(server.server_port))
server.serve_forever()
