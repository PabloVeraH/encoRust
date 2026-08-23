# mp3-wasm streaming harness

Minimal hand-written JS/HTML harness for the M9 Definition of Done
(`docs/mp3-encoder/12-phase9-cli-and-wasm.md` §4): pushes PCM chunks
smaller than one MP3 frame into `WasmEncoder` and confirms
correctly-framed MP3 bytes come back out, then plays the result back
through a real `<audio>` element (i.e. the browser's own MP3 decoder).

## Build and run

```sh
# From the repo root:
cd crates/mp3-wasm
wasm-pack build --target web --out-dir examples/streaming-harness/pkg

# Then serve this directory over HTTP (opening index.html directly via
# file:// will NOT work -- ES module imports and the wasm fetch both
# require an http(s) origin):
cd examples/streaming-harness
python3 -m http.server 8000
# open http://localhost:8000/ and click "Run streaming encode"
```

## Known limitation

`wasm-pack` is not installed in the environment this harness was
written and reviewed in, so it has been read through carefully against
`WasmEncoder`'s actual public API (constructor + `push`/`finish`
signatures, `Result<Vec<u8>, JsValue>` returns surfacing as thrown JS
exceptions on error per wasm-bindgen's standard codegen) but has **not**
been executed end-to-end in a real browser here. Build and open it
locally to verify — this is the same class of external-tooling gap
already disclosed for M8/M9 (no ffmpeg, no Symphonia harness, no
`wasm-pack`/`wasm-bindgen-cli` available for this review).
