//! Screenshot and GIF capture: encode the simulation's rendered grid to a PNG
//! or an animated GIF and hand it to the user — written to a file on the
//! desktop, or offered as a browser download on the web.
//!
//! The frame source is the very same tightly-packed RGBA buffer the renderer
//! uploads each tick (see [`crate::sim::Simulation::render_into`]), so a capture
//! is exactly what the grid holds — minus the GPU bloom halo and the egui panel.
//! Both encoders are pure Rust, so this one module serves both platforms; only
//! [`deliver`] forks (filesystem vs. browser blob).

// ---- Bloom (CPU) -----------------------------------------------------------
//
// On screen, emissive materials (fire, lava, meteors) don't show their stored
// base colour — they go through the GPU's selective bloom (see `shader.wgsl`),
// which is what turns fire's muddy, heavily-jittered orange into a bright glow.
// A capture reads the *raw grid* the GPU uploads, so without reproducing that
// bloom here fire comes out dim and brown. These constants mirror the GPU path
// exactly so a capture matches what the player sees: `shader.wgsl` (RADIUS /
// TAPS / GLOW_STRENGTH) and `gpu.rs` (GLOW_SPREAD).
const BLOOM_RADIUS: i32 = 4;
const BLOOM_TAPS: f32 = 9.0; // 2*RADIUS + 1
const BLOOM_STRENGTH: f32 = 1.4;
const BLOOM_SPREAD: f32 = 1.5; // texels between blur taps

/// sRGB-encoded byte → linear light. The grid texture is sRGB, so the GPU does
/// the bloom maths in linear space; we must too, or the added glow shifts hue.
fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light → sRGB-encoded byte (the inverse of [`srgb_to_linear`]).
fn linear_to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let s = if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

/// Apply the renderer's selective bloom to a raw grid frame and return an opaque
/// RGBA image. [`render_into`](crate::sim::Simulation::render_into) repurposes
/// the alpha channel as a glow flag (`0` = emissive, `255` = opaque), so a raw
/// frame both has the wrong alpha *and* lacks the halo; this resolves both.
fn bloom(frame: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let n = w * h;

    // Decode to linear: the full scene, plus a glow source that keeps only the
    // emissive pixels (alpha 0) — exactly the `step(a, 0.0)` mask in `fs_blur_h`.
    let mut scene = vec![[0f32; 3]; n];
    let mut glow = vec![[0f32; 3]; n];
    for i in 0..n {
        let p = &frame[i * 4..i * 4 + 4];
        let lin = [
            srgb_to_linear(p[0]),
            srgb_to_linear(p[1]),
            srgb_to_linear(p[2]),
        ];
        scene[i] = lin;
        if p[3] == 0 {
            glow[i] = lin;
        }
    }

    // Separable box blur of the glow, taps spaced BLOOM_SPREAD texels apart and
    // clamped to the edge (matching the GPU's nearest sampler + ClampToEdge).
    let offsets: Vec<i32> = (-BLOOM_RADIUS..=BLOOM_RADIUS)
        .map(|k| (k as f32 * BLOOM_SPREAD).round() as i32)
        .collect();
    let blur_axis = |src: &[[f32; 3]], horizontal: bool| -> Vec<[f32; 3]> {
        let mut out = vec![[0f32; 3]; n];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0f32; 3];
                for &o in &offsets {
                    let (sx, sy) = if horizontal {
                        ((x as i32 + o).clamp(0, w as i32 - 1) as usize, y)
                    } else {
                        (x, (y as i32 + o).clamp(0, h as i32 - 1) as usize)
                    };
                    let s = src[sy * w + sx];
                    acc[0] += s[0];
                    acc[1] += s[1];
                    acc[2] += s[2];
                }
                out[y * w + x] = [
                    acc[0] / BLOOM_TAPS,
                    acc[1] / BLOOM_TAPS,
                    acc[2] / BLOOM_TAPS,
                ];
            }
        }
        out
    };
    let blurred = blur_axis(&blur_axis(&glow, true), false);

    // Composite: crisp scene + blurred glow, back to opaque sRGB bytes.
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let s = scene[i];
        let g = blurred[i];
        out[i * 4] = linear_to_srgb(s[0] + g[0] * BLOOM_STRENGTH);
        out[i * 4 + 1] = linear_to_srgb(s[1] + g[1] * BLOOM_STRENGTH);
        out[i * 4 + 2] = linear_to_srgb(s[2] + g[2] * BLOOM_STRENGTH);
        out[i * 4 + 3] = 255;
    }
    out
}

/// Encode a single grid frame (`w * h * 4` RGBA bytes) as a PNG.
pub fn encode_png(frame: &[u8], w: u32, h: u32) -> Vec<u8> {
    let rgba = bloom(frame, w, h);
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().expect("write png header");
        writer.write_image_data(&rgba).expect("write png data");
    }
    buf
}

/// Chroma (max − min channel) above which a colour counts as "vivid" and is
/// up-weighted when building the GIF palette, with a stronger boost past the
/// second threshold. Fire/lava/meteor oranges sit well above both; dirt and
/// foliage sit around/below the first, the gray terrain far below.
const VIVID_CHROMA: i32 = 70;
const VERY_VIVID_CHROMA: i32 = 120;

/// Palette index reserved as the inter-frame "transparent = unchanged" slot.
/// The quantiser only ever assigns real colours to indices `0..=254`, so 255 is
/// free to mean "leave whatever the previous frame drew here" (see
/// [`encode_gif`]).
const TRANSPARENT_INDEX: u8 = 255;

/// Encode a run of grid frames as a looping animated GIF at `delay_cs`
/// centiseconds per frame.
///
/// Two things make this more than a naive frame dump:
///
/// 1. **One shared, vivid-aware palette.** GIF caps a frame at 256 colours, and
///    a frequency-weighted quantiser (what `gif`'s per-frame `from_rgba_speed`
///    uses) spends them on whatever covers the most pixels — sky, gray stone,
///    dirt — mapping the few hundred vivid fire/meteor pixels onto the nearest
///    survivor, which is dirt brown. So we build the palette from a sample that
///    *repeats* vivid pixels (see [`VIVID_CHROMA`]), buying them entries.
///
/// 2. **Inter-frame differencing.** Most of the world is static terrain whose
///    per-cell jitter is high-entropy noise that barely LZW-compresses, so a
///    full every-pixel frame is huge. Instead, after the first frame we emit
///    only pixels that *changed*, marking the rest transparent over a "keep the
///    canvas" disposal — so unchanged terrain costs almost nothing. This is what
///    makes our files a fraction of their former size.
pub fn encode_gif(frames: &[Vec<u8>], w: u32, h: u32, delay_cs: u16) -> Vec<u8> {
    // Render each frame to its on-screen appearance (bloom + opaque) up front;
    // both the palette and the per-frame mapping read these.
    let bloomed: Vec<Vec<u8>> = frames.iter().map(|f| bloom(f, w, h)).collect();
    if bloomed.is_empty() {
        return Vec::new();
    }

    let nq = build_palette(&bloomed);
    // 255 real colours (indices 0..=254); pad the global palette to a full 256
    // entries so TRANSPARENT_INDEX (255) is a valid, unused slot.
    let mut pal_rgb: Vec<u8> = nq
        .color_map_rgba()
        .chunks_exact(4)
        .flat_map(|c| [c[0], c[1], c[2]])
        .collect();
    pal_rgb.resize(256 * 3, 0);
    // `index_of` is a relatively costly nearest-colour search; precompute it once
    // into an RGB→index table so the per-pixel mapping below is a array lookup.
    let mapper = Mapper::new(&nq);

    let mut buf = Vec::new();
    {
        let mut enc =
            gif::Encoder::new(&mut buf, w as u16, h as u16, &pal_rgb).expect("create gif encoder");
        enc.set_repeat(gif::Repeat::Infinite)
            .expect("set gif repeat");
        let mut prev: Option<&Vec<u8>> = None;
        for frame in &bloomed {
            // First frame: map every pixel. Later frames: keep only what changed
            // from the previous frame and mark the rest transparent.
            let indices: Vec<u8> = match prev {
                None => frame.chunks_exact(4).map(|px| mapper.index(px)).collect(),
                Some(p) => frame
                    .chunks_exact(4)
                    .zip(p.chunks_exact(4))
                    .map(|(px, pp)| {
                        if px == pp {
                            TRANSPARENT_INDEX
                        } else {
                            mapper.index(px)
                        }
                    })
                    .collect(),
            };
            let mut f = gif::Frame::default();
            f.width = w as u16;
            f.height = h as u16;
            f.buffer = std::borrow::Cow::Owned(indices);
            f.delay = delay_cs;
            // Keep the canvas so transparent (unchanged) pixels show the prior
            // frame through.
            f.dispose = gif::DisposalMethod::Keep;
            if prev.is_some() {
                f.transparent = Some(TRANSPARENT_INDEX);
            }
            enc.write_frame(&f).expect("write gif frame");
            prev = Some(frame);
        }
    }
    buf
}

/// Train a NeuQuant palette of 255 real colours (the 256th index is reserved for
/// transparency), repeating vivid pixels so they earn palette entries instead of
/// collapsing onto the common dull colours.
///
/// Adjacent frames are nearly identical, so the palette is trained on only a
/// handful of frames sampled across the clip, and strided within each, to keep
/// training fast regardless of how long the recording is.
fn build_palette(bloomed: &[Vec<u8>]) -> color_quant::NeuQuant {
    // At most this many frames feed the palette; spread them across the clip.
    const MAX_FRAMES: usize = 16;
    // Target number of (pre-upweight) pixels in the training sample.
    const TARGET_PX: usize = 150_000;

    let frame_step = (bloomed.len() / MAX_FRAMES).max(1);
    let chosen: Vec<&Vec<u8>> = bloomed.iter().step_by(frame_step).collect();
    let chosen_px: usize = chosen.iter().map(|f| f.len() / 4).sum();
    let stride = (chosen_px / TARGET_PX).max(1);

    let mut sample: Vec<u8> = Vec::new();
    for frame in chosen {
        for px in frame.chunks_exact(4).step_by(stride) {
            let max = px[0].max(px[1]).max(px[2]) as i32;
            let min = px[0].min(px[1]).min(px[2]) as i32;
            let chroma = max - min;
            let copies = if chroma > VERY_VIVID_CHROMA {
                24
            } else if chroma > VIVID_CHROMA {
                4
            } else {
                1
            };
            for _ in 0..copies {
                sample.extend_from_slice(px);
            }
        }
    }
    // samplefac 1 = highest-quality pass; the sample is already small and bounded.
    // 255 colours, leaving index 255 free as the transparent slot.
    color_quant::NeuQuant::new(1, 255, &sample)
}

/// A precomputed RGB→palette-index table. NeuQuant's `index_of` is a nearest-
/// colour search; doing it per pixel over a whole recording is slow, so we run
/// it once per cell of a coarse 5-bit-per-channel cube (32³ = 32 768 entries) and
/// then map each pixel with a single array lookup. The slight extra quantisation
/// is invisible against the 255-colour palette.
struct Mapper {
    lut: Vec<u8>,
}

impl Mapper {
    fn new(nq: &color_quant::NeuQuant) -> Self {
        let mut lut = vec![0u8; 32 * 32 * 32];
        for r in 0..32u32 {
            for g in 0..32u32 {
                for b in 0..32u32 {
                    // Expand the 5-bit cell back across the full 0..=255 range.
                    let expand = |v: u32| ((v << 3) | (v >> 2)) as u8;
                    let rgba = [expand(r), expand(g), expand(b), 255];
                    lut[(r * 1024 + g * 32 + b) as usize] = nq.index_of(&rgba) as u8;
                }
            }
        }
        Mapper { lut }
    }

    #[inline]
    fn index(&self, px: &[u8]) -> u8 {
        let r = (px[0] >> 3) as usize;
        let g = (px[1] >> 3) as usize;
        let b = (px[2] >> 3) as usize;
        self.lut[r * 1024 + g * 32 + b]
    }
}

/// A unique, sortable filename like `sandy-1718559000123.png`. `web_time` gives
/// a wall-clock timestamp on both desktop and wasm (plain `SystemTime` panics in
/// the browser).
pub fn filename(ext: &str) -> String {
    let stamp = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("sandy-{stamp}.{ext}")
}

/// Save encoded bytes to the working directory.
#[cfg(not(target_arch = "wasm32"))]
pub fn deliver(bytes: &[u8], filename: &str, _mime: &str) {
    match std::fs::write(filename, bytes) {
        Ok(()) => log::info!("saved {filename} ({} bytes)", bytes.len()),
        Err(e) => log::error!("failed to save {filename}: {e}"),
    }
}

/// Offer encoded bytes to the browser as a download: wrap them in a Blob, point
/// a throwaway `<a download>` at an object URL, and click it.
#[cfg(target_arch = "wasm32")]
pub fn deliver(bytes: &[u8], filename: &str, _mime: &str) {
    use wasm_bindgen::JsCast;

    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::of1(&array);
    let blob = match web_sys::Blob::new_with_u8_array_sequence(&parts) {
        Ok(b) => b,
        Err(e) => {
            log::error!("capture: failed to build blob: {e:?}");
            return;
        }
    };
    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(e) => {
            log::error!("capture: failed to create object url: {e:?}");
            return;
        }
    };

    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        log::error!("capture: no document");
        return;
    };
    let anchor = document
        .create_element("a")
        .ok()
        .and_then(|el| el.dyn_into::<web_sys::HtmlAnchorElement>().ok());
    let Some(anchor) = anchor else {
        log::error!("capture: failed to create download anchor");
        return;
    };
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();
    // The browser has the blob now; release the object URL so it isn't leaked.
    let _ = web_sys::Url::revoke_object_url(&url);
    log::info!("downloaded {filename} ({} bytes)", bytes.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a frame: dark opaque background with a central disk of emissive,
    /// heavily-jittered "fire" cells (alpha 0), like the sim produces.
    fn fire_frame(w: u32, h: u32) -> Vec<u8> {
        let (w, h) = (w as usize, h as usize);
        let mut seed: u32 = 7;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        };
        let mut f = vec![0u8; w * h * 4];
        let (cx, cy, r) = (w as i32 / 2, h as i32 / 2, 16);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let d2 = (x as i32 - cx).pow(2) + (y as i32 - cy).pow(2);
                if d2 < r * r {
                    // Fire: [240,120,30], jitter 64, glow flag (alpha 0).
                    let v = rnd() as i32;
                    let off = (v - 128) * 64 / 128;
                    let ch = |c: i32| (c + off).clamp(0, 255) as u8;
                    f[i..i + 4].copy_from_slice(&[ch(240), ch(120), ch(30), 0]);
                } else {
                    f[i..i + 4].copy_from_slice(&[18, 18, 22, 255]); // dim stone-ish bg
                }
            }
        }
        f
    }

    #[test]
    fn bloom_makes_fire_glow_not_brown() {
        let (w, h) = (64u32, 64u32);
        let frame = fire_frame(w, h);
        let out = bloom(&frame, w, h);

        // Centre pixel: should stay orange (R > G > B) and be bright, not the
        // muddy brown a raw dim emitter shows.
        let c = ((h as usize / 2) * w as usize + w as usize / 2) * 4;
        let (r, g, b) = (out[c] as i32, out[c + 1] as i32, out[c + 2] as i32);
        assert!(r > g && g > b, "fire hue lost: {r},{g},{b}");
        assert!(r > 230, "fire not bright enough: R={r}");

        // The halo must bleed past the disk: a pixel just outside the r=16 disk
        // (e.g. 20px out) should be lifted above the background by the glow.
        let edge = ((h as usize / 2) * w as usize + (w as usize / 2 + 20)) * 4;
        assert!(
            out[edge] as i32 > 30,
            "no halo outside emitter: {}",
            out[edge]
        );

        // A non-emissive pixel well clear of the halo is untouched (still bg).
        let bg = (2 * w as usize + 2) * 4;
        assert_eq!(
            (out[bg], out[bg + 1], out[bg + 2], out[bg + 3]),
            (18, 18, 22, 255)
        );
    }

    /// A scene like the real captures: mostly sky / stone / trees / dirt, with a
    /// few scattered emissive orange meteor specks (alpha 0).
    fn scene_with_meteors(w: usize, h: usize) -> Vec<u8> {
        let mut seed: u32 = 99;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        };
        let mut f = vec![0u8; w * h * 4];
        let mut put = |f: &mut [u8], x: usize, y: usize, c: [u8; 4]| {
            let i = (y * w + x) * 4;
            f[i..i + 4].copy_from_slice(&c);
        };
        for y in 0..h {
            for x in 0..w {
                let ground = 110 + ((x as f32 * 0.05).sin() * 20.0) as i32;
                let jit = |base: i32, j: i32, v: u8| {
                    (base + (v as i32 - 128) * j / 128).clamp(0, 255) as u8
                };
                let c = if (y as i32) < ground {
                    [120, 170, 220, 255]
                } else if (y as i32) < ground + 6 {
                    [
                        jit(110, 18, rnd()),
                        jit(75, 18, rnd()),
                        jit(45, 18, rnd()),
                        255,
                    ]
                } else {
                    [
                        jit(128, 18, rnd()),
                        jit(128, 18, rnd()),
                        jit(134, 18, rnd()),
                        255,
                    ]
                };
                put(&mut f, x, y, c);
            }
        }
        // Scatter emissive specks down through the dirt/stone band (y 90..170),
        // where they must compete with the abundant brown — the real failure
        // case, not orange-against-far-away-blue-sky.
        for _ in 0..600 {
            let x = 200 + (rnd() as usize) % 120;
            let y = 95 + (rnd() as usize) % 70;
            if y >= h {
                continue;
            }
            // Meteor [255,170,60] jitter 40, emissive (alpha 0).
            put(
                &mut f,
                x,
                y,
                [jit_(255, rnd()), jit_(170, rnd()), jit_(60, rnd()), 0],
            );
        }
        f
    }
    fn jit_(base: i32, v: u8) -> u8 {
        (base + (v as i32 - 128) * 40 / 128).clamp(0, 255) as u8
    }

    /// The GIF palette must keep the (rare) meteor orange recognisably orange,
    /// not collapse it onto the abundant dirt brown. Writes the GIF to /tmp when
    /// SANDY_DUMP is set, for eyeballing.
    #[test]
    fn meteors_survive_gif_quantization() {
        let (w, h) = (500usize, 250usize);
        let frame = scene_with_meteors(w, h);
        let gif = encode_gif(&[frame], w as u32, h as u32, 7);

        // Decode the GIF back and look at what the meteor region became.
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = opts.read_info(std::io::Cursor::new(&gif)).unwrap();
        let decoded = decoder.read_next_frame().unwrap().unwrap();
        let buf = &decoded.buffer;

        // Count, over the meteor region, how many warm (orange) vs brown pixels
        // there are. Orange = R high, G mid, B low, with a wide R−B gap; the
        // failure mode is everything snapping to dirt (R−B small, all dim).
        let mut warm = 0;
        let mut total = 0;
        for y in 95..165 {
            for x in 200..320 {
                let i = (y * w + x) * 4;
                let (r, g, b) = (buf[i] as i32, buf[i + 1] as i32, buf[i + 2] as i32);
                total += 1;
                // Warm/orange: clearly red-dominant and bright, well past where
                // dirt brown (R−B ≈ 65) sits.
                if r > 190 && r - b > 100 && r > g && g > b {
                    warm += 1;
                }
            }
        }
        if std::env::var("SANDY_DUMP").is_ok() {
            std::fs::write("/tmp/qtest.gif", &gif).unwrap();
        }
        // ~600 emissive specks were placed in this region. With per-frame
        // quantisation essentially none survive as orange (they map to dirt);
        // with the saturation-weighted global palette the bulk come through.
        assert!(
            warm > 400,
            "meteors lost to quantization: only {warm} warm pixels in region (of ~600 placed)"
        );
    }

    /// Inter-frame differencing: a clip whose frames don't change should cost
    /// barely more than a single frame, and must still decode back to the
    /// original frames (transparent pixels filled in from the kept canvas).
    #[test]
    fn static_frames_difference_to_almost_nothing() {
        let (w, h) = (500usize, 250usize);
        let frame = scene_with_meteors(w, h);

        let one = encode_gif(std::slice::from_ref(&frame), w as u32, h as u32, 7);
        let many = encode_gif(&vec![frame.clone(); 30], w as u32, h as u32, 7);

        // 29 extra identical frames are all-transparent, so they add very little:
        // the 30-frame clip must be far smaller than 30 independent frames would
        // be (which is what naive full-frame encoding produced).
        assert!(
            many.len() < one.len() * 2,
            "differencing ineffective: 1 frame = {} bytes, 30 frames = {} bytes",
            one.len(),
            many.len()
        );

        // And it must still be correct: decode + composite (apply Keep disposal)
        // and confirm every frame matches the first.
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = opts.read_info(std::io::Cursor::new(&many)).unwrap();
        let mut canvas = vec![0u8; w * h * 4];
        let mut first: Option<Vec<u8>> = None;
        let mut count = 0;
        while let Some(fr) = decoder.read_next_frame().unwrap() {
            // Composite: non-transparent (alpha != 0) pixels overwrite the canvas.
            for (c, p) in canvas.chunks_exact_mut(4).zip(fr.buffer.chunks_exact(4)) {
                if p[3] != 0 {
                    c.copy_from_slice(p);
                }
            }
            match &first {
                None => first = Some(canvas.clone()),
                Some(f) => assert_eq!(&canvas, f, "frame {count} differs after compositing"),
            }
            count += 1;
        }
        assert_eq!(count, 30);
    }
}
