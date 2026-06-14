set shell := ["zsh", "-cu"]

width := "15360"
height := "8640"
iterations := "1000000000"
palette := "aurora"
palettes := "dusk ember aurora"
threads := "auto"
raw := "out/16k.ppm"
png := "out/16k.png"

default: frame

build:
    cargo build --release

frame:
    mkdir -p out
    cargo run --release -- \
      --width {{width}} \
      --height {{height}} \
      --iterations {{iterations}} \
      --palette {{palette}} \
      {{ if threads == "auto" { "" } else { "--threads " + threads } }} \
      --out {{raw}}
    ffmpeg -y -i {{raw}} {{png}}
    rm -f {{raw}}
    @echo {{png}}

palettes:
    mkdir -p out/palettes
    for scheme in {{palettes}}; do \
      cargo run --release -- \
        --width {{width}} \
        --height {{height}} \
        --iterations {{iterations}} \
        --palette $scheme \
        {{ if threads == "auto" { "" } else { "--threads " + threads } }} \
        --out out/palettes/$scheme.ppm; \
      ffmpeg -y -i out/palettes/$scheme.ppm out/palettes/$scheme.png; \
      rm -f out/palettes/$scheme.ppm; \
      echo out/palettes/$scheme.png; \
    done

clean:
    rm -rf target
