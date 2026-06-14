# attractors

Small Rust Clifford attractor renderer.

```sh
just                  # render out/16k.png
just build            # build the release binary
just frame            # render out/16k.png
just palettes         # render out/palettes/{dusk,ember,aurora}.png
just clean            # remove target

just threads=8 frame
just width=7680 height=4320 iterations=100000000 frame
```
