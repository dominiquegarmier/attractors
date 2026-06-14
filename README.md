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

<img width="1920" height="1080" alt="hd" src="https://github.com/user-attachments/assets/761e4275-86e2-423b-91fc-f5257f287479" />
