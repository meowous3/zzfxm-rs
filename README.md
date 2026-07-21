# zzfxm-rs

[![CI](https://github.com/meowous3/zzfxm-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/meowous3/zzfxm-rs/actions/workflows/ci.yml)

A faithful, unofficial Rust port of [ZzFXM](https://github.com/keithclark/ZzFXM), the tiny tracker-style music renderer by Keith Clark and Frank Force.

`zzfxm-rs` turns ZzFX instruments, reusable patterns, a sequence, and a BPM into stereo `f32` PCM. It uses [`zzfx-rs`](https://github.com/meowous3/zzfx-rs) for ZzFXM-compatible instrument synthesis and leaves playback to the application.

## Features

- ZzFXM 2.0.3-compatible sequencing and mixing
- Stereo panning
- Fractional note attenuation
- Note release and click-prevention fades
- Instrument/note sample caching
- Compact nested-array conversion with JavaScript-hole support
- Caller-provided randomness for deterministic rendering
- Optional Serde support
- No playback-backend dependency

## Installation

Until the crate is published to crates.io, install it from GitHub:

```toml
[dependencies]
zzfxm-rs = { git = "https://github.com/meowous3/zzfxm-rs" }
```

Enable Serde derives with:

```toml
zzfxm-rs = { git = "https://github.com/meowous3/zzfxm-rs", features = ["serde"] }
```

## Usage

```rust
use zzfx_rs::ZzfxParams;
use zzfxm_rs::{Channel, Pattern, Renderer, Song};

let instrument = ZzfxParams::new(0.5, 440.0, 0.01, 0.2, 0.3);
let pattern = Pattern::new(vec![Channel::new(
    0,                    // instrument
    0.0,                  // center pan
    vec![12.0, 0.0, 16.5, 0.0],
)]);
let song = Song::new(vec![instrument], vec![pattern], vec![0, 0], 120.0);

let stereo = Renderer::default().render(&song)?;
println!("rendered {:.2} seconds", stereo.duration_seconds());
# Ok::<(), zzfxm_rs::SongError>(())
```

A note's integer portion selects pitch. Its fractional portion controls attenuation, `0` is a rest, and a negative value releases the current note. Pan ranges from `-1` for left to `1` for right.

## Compact ZzFXM data

`Song::from_compact` accepts the nested-array structure used by JavaScript ZzFXM songs. Represent omitted JavaScript array entries as `None`:

```rust
use zzfxm_rs::Song;

let song = Song::from_compact(
    vec![vec![Some(0.5), None, Some(440.0)]],
    vec![vec![vec![None, Some(0.0), Some(12.0), None]]],
    vec![0],
    Some(125.0),
)?;
# Ok::<(), zzfxm_rs::SongError>(())
```

## Compatibility

The renderer follows ZzFXM 2.0.3, including its beat timing, channel mixing, panning, attenuation, end-of-note fades, and legacy ZzFX instrument behavior. Regression samples are compared against the original JavaScript implementation.

## Development

```sh
cargo test --all-features
```

## Relationship to ZzFXM

This is an independent Rust port, not an official ZzFXM project. Please report Rust-port issues here and upstream ZzFXM issues to the [original project](https://github.com/keithclark/ZzFXM).

## License

MIT. This is a derivative port of ZzFXM; the original copyright and license are retained in [LICENSE](LICENSE).
