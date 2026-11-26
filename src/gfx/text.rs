use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use cosmic_text::{
    Attrs, Buffer, CacheKey, CacheKeyFlags, Family, FontSystem, Metrics, Shaping, Style,
    SwashCache, SwashContent, Weight, Wrap,
};

use crate::ui::context::TextMeasure;
use crate::ui::draw::{OwnedSpan, SpanKind};
use crate::ui::types::{Color, Rect, TextAlign};

type FontEntry = (String, Arc<Vec<u8>>);
static REGISTERED_FONTS: OnceLock<Mutex<Vec<FontEntry>>> = OnceLock::new();
static DEFAULT_SANS_FAMILY: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static DEFAULT_MONO_FAMILY: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// Registers raw TTF/OTF/TTC font bytes globally under `name` for all existing and future windows.
pub fn register_font(name: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let reg = REGISTERED_FONTS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut list) = reg.lock() {
        list.push((String::from(name.trim()), Arc::new(bytes.to_vec())));
    }
}

/// Registers raw TTF/OTF/TTC font bytes globally using the font's embedded OpenType family names.
pub fn register_font_bytes(bytes: &[u8]) {
    register_font("", bytes);
}

/// Sets the global default sans-serif UI font family (system name or registered font alias).
pub fn set_default_font_family(name: &str) {
    let trimmed = name.trim();
    let slot = DEFAULT_SANS_FAMILY.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = (!trimmed.is_empty()).then(|| String::from(trimmed));
    }
}

/// Sets the global default monospace UI font family (system name or registered font alias).
pub fn set_default_mono_font_family(name: &str) {
    let trimmed = name.trim();
    let slot = DEFAULT_MONO_FAMILY.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = (!trimmed.is_empty()).then(|| String::from(trimmed));
    }
}

/// Subsetted open-source fallback font (< 45 KB) embedded for headless and WASM rendering.
pub const FALLBACK_FONT_BYTES: &[u8] = include_bytes!("../../assets/fallback.ttf");

const INITIAL_ATLAS_SIZE: u32 = 512;
const MAX_ATLAS_SIZE: u32 = 2048;
const MAX_TEXT_CACHE_ENTRIES: usize = 512;

/// Dirty atlas region returned by [`GlyphAtlas::take_dirty_upload_region`].
pub enum AtlasUpload<'a> {
    /// Full texture allocation/upload (`glTexImage2D`).
    Full {
        width: u32,
        height: u32,
        pixels: &'a [u8],
    },
    /// Partial row-band update (`glTexSubImage2D`) when atlas dimensions did not change.
    SubRows {
        atlas_width: u32,
        atlas_height: u32,
        y: u32,
        height: u32,
        pixels: &'a [u8],
    },
}

#[derive(Clone, Copy, Debug)]
struct CachedShapedGlyph {
    rel_x: f32,
    rel_y: f32,
    w: f32,
    h: f32,
    atlas_px: [u32; 4],
    is_color: bool,
}

struct DualHash {
    h1: std::collections::hash_map::DefaultHasher,
    h2: u64,
}

impl DualHash {
    fn new() -> Self {
        Self {
            h1: std::collections::hash_map::DefaultHasher::new(),
            h2: 0xcbf2_9ce4_8422_2325,
        }
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        use std::hash::Hasher;
        self.h1.write(bytes);
        for &b in bytes {
            self.h2 ^= u64::from(b);
            self.h2 = self.h2.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Domain separator so adjacent slices cannot alias.
        self.h2 ^= 0xff;
        self.h2 = self.h2.wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }

    fn finish(self) -> (u64, u64) {
        use std::hash::Hasher;
        (self.h1.finish(), self.h2)
    }
}

#[derive(Clone, Copy, Debug)]
struct CachedGlyph {
    px_x: u32,
    px_y: u32,
    px_w: u32,
    px_h: u32,
    left: i32,
    top: i32,
    is_color: bool,
}

#[derive(Clone, Copy, Debug)]
enum GlyphLookup {
    Ready(CachedGlyph),
    Empty,
    Overflow,
}

/// Shaped glyph quad in physical framebuffer pixels paired with integer atlas texel bounds.
#[derive(Clone, Copy, Debug)]
pub struct ShapedGlyphQuad {
    pub phys_rect: [f32; 4],
    pub atlas_px: [u32; 4],
    pub color: Color,
}

/// Additional decoration rects emitted by rich-text shaping (pills and link underlines).
#[derive(Clone, Copy, Debug)]
pub struct RichDecorationRect {
    pub phys_rect: [f32; 4],
    pub color: Color,
    pub radius: f32,
}

/// Output of shaping a plain or rich text command.
#[derive(Clone, Debug, Default)]
pub struct ShapedTextOutput {
    pub bg_rects: Vec<RichDecorationRect>,
    pub glyphs: Vec<ShapedGlyphQuad>,
    pub fg_rects: Vec<RichDecorationRect>,
}

/// Dynamic CPU-to-GPU RGBA8 glyph atlas backed by `cosmic-text`.
pub struct GlyphAtlas {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas_w: u32,
    atlas_h: u32,
    pixels: Vec<u8>,
    cursor_x: u32,
    cursor_y: u32,
    shelf_h: u32,
    used_area: u64,
    cache: HashMap<CacheKey, Option<CachedGlyph>>,
    has_italic_face: bool,
    dirty_upload: bool,
    dirty_full: bool,
    dirty_y_range: Option<(u32, u32)>,
    registered_count: usize,
    font_aliases: HashMap<String, String>,
    applied_default_sans: Option<String>,
    applied_default_mono: Option<String>,
    scratch_buf: Buffer,
    measure_cache: HashMap<(u64, u64), [f32; 2]>,
    shape_cache: HashMap<(u64, u64), Vec<CachedShapedGlyph>>,
    atlas_epoch: u64,
    batch_defer_reset: bool,
    batch_overflowed: bool,
}

impl GlyphAtlas {
    /// Creates a fast atlas loaded solely with the embedded fallback font.
    pub fn new_fallback_only() -> Self {
        Self::build(false)
    }

    /// Creates an atlas loading native system fonts (when available) plus the embedded fallback.
    pub fn new_with_system_fonts() -> Self {
        Self::build(true)
    }

    fn build(load_system: bool) -> Self {
        let mut db = cosmic_text::fontdb::Database::new();
        #[cfg(not(target_arch = "wasm32"))]
        if load_system {
            db.load_system_fonts();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = load_system;

        db.load_font_source(cosmic_text::fontdb::Source::Binary(Arc::new(
            FALLBACK_FONT_BYTES.to_vec(),
        )));

        if !load_system || db.faces().count() <= 1 {
            db.set_sans_serif_family("LumenicSans");
            db.set_serif_family("LumenicSans");
            db.set_monospace_family("LumenicSans");
        }
        let has_italic_face = db.faces().any(|f| f.style == Style::Italic);

        let mut font_system = FontSystem::new_with_locale_and_db(String::from("en-US"), db);
        let swash_cache = SwashCache::new();

        let cap = usize::try_from(INITIAL_ATLAS_SIZE)
            .ok()
            .and_then(|w| w.checked_mul(w))
            .and_then(|px| px.checked_mul(4))
            .expect("initial atlas capacity fits");
        let mut pixels = vec![0u8; cap];

        // Reserve 2x2 solid white texels at (0, 0) for non-textured SDF geometry if sampled.
        for y in 0..2u32 {
            for x in 0..2u32 {
                let idx = ((y as usize) * (INITIAL_ATLAS_SIZE as usize) + (x as usize)) * 4;
                pixels[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }

        let scratch_buf = Buffer::new(&mut font_system, Metrics::new(14.0, 18.9));
        let mut atlas = Self {
            font_system,
            swash_cache,
            atlas_w: INITIAL_ATLAS_SIZE,
            atlas_h: INITIAL_ATLAS_SIZE,
            pixels,
            cursor_x: 3,
            cursor_y: 1,
            shelf_h: 2,
            used_area: 4,
            cache: HashMap::new(),
            has_italic_face,
            dirty_upload: true,
            dirty_full: true,
            dirty_y_range: None,
            registered_count: 0,
            font_aliases: HashMap::new(),
            applied_default_sans: None,
            applied_default_mono: None,
            scratch_buf,
            measure_cache: HashMap::new(),
            shape_cache: HashMap::new(),
            atlas_epoch: 0,
            batch_defer_reset: false,
            batch_overflowed: false,
        };
        atlas.sync_registered_fonts();
        atlas
    }

    fn sync_registered_fonts(&mut self) {
        if let Some(reg) = REGISTERED_FONTS.get() {
            if let Ok(list) = reg.lock() {
                if list.len() > self.registered_count {
                    let pending = list[self.registered_count..].to_vec();
                    let new_total = list.len();
                    drop(list);

                    let db = self.font_system.db_mut();
                    for (alias, bytes) in pending {
                        let before = db.faces().count();
                        db.load_font_source(cosmic_text::fontdb::Source::Binary(bytes));
                        let mut first_actual: Option<String> = None;
                        for face in db.faces().skip(before) {
                            for (actual_name, _) in &face.families {
                                if first_actual.is_none() {
                                    first_actual = Some(actual_name.clone());
                                }
                                self.font_aliases.insert(
                                    actual_name.to_ascii_lowercase(),
                                    actual_name.clone(),
                                );
                            }
                        }
                        if !alias.is_empty() {
                            if let Some(actual_name) = first_actual {
                                self.font_aliases
                                    .insert(alias.to_ascii_lowercase(), actual_name);
                            }
                        }
                    }
                    self.has_italic_face = db.faces().any(|f| f.style == Style::Italic);
                    self.registered_count = new_total;
                    self.reset_atlas();
                    self.measure_cache.clear();
                }
            }
        }

        let raw_sans = DEFAULT_SANS_FAMILY
            .get()
            .and_then(|m| m.lock().ok().and_then(|g| g.clone()));
        let desired_sans = self.resolve_family_owned(raw_sans.as_deref());
        if desired_sans != self.applied_default_sans {
            if let Some(ref fam) = desired_sans {
                let db = self.font_system.db_mut();
                db.set_sans_serif_family(fam);
                db.set_serif_family(fam);
            }
            self.applied_default_sans = desired_sans;
            self.reset_atlas();
            self.measure_cache.clear();
        }

        let raw_mono = DEFAULT_MONO_FAMILY
            .get()
            .and_then(|m| m.lock().ok().and_then(|g| g.clone()));
        let desired_mono = self.resolve_family_owned(raw_mono.as_deref());
        if desired_mono != self.applied_default_mono {
            if let Some(ref fam) = desired_mono {
                self.font_system.db_mut().set_monospace_family(fam);
            }
            self.applied_default_mono = desired_mono;
            self.reset_atlas();
            self.measure_cache.clear();
        }
    }

    fn resolve_family_owned(&self, family: Option<&str>) -> Option<String> {
        let raw = family?.trim();
        if raw.is_empty() {
            return None;
        }
        if let Some(mapped) = self.font_aliases.get(&raw.to_ascii_lowercase()) {
            return Some(mapped.clone());
        }
        Some(String::from(raw))
    }

    fn effective_family_owned(&self, family: Option<&str>, mono: bool) -> Option<String> {
        if let Some(explicit) = self.resolve_family_owned(family) {
            return Some(explicit);
        }
        if mono {
            self.applied_default_mono.clone()
        } else {
            self.applied_default_sans.clone()
        }
    }

    // Snap requested weight to the nearest Normal face in the target family so
    // cosmic-text fallback never jumps to unrelated symbol/CJK fonts (e.g. Marlett at weight 500).
    fn resolve_weight_for_family(
        &self,
        resolved_family: Option<&str>,
        mono: bool,
        weight: u16,
    ) -> u16 {
        let target = weight.clamp(100, 900);
        let db = self.font_system.db();
        let default_fam: Option<String> = if resolved_family.is_none() {
            let generic = if mono {
                Family::Monospace
            } else {
                Family::SansSerif
            };
            let q = cosmic_text::fontdb::Query {
                families: &[generic],
                ..Default::default()
            };
            db.query(&q)
                .and_then(|id| db.face(id))
                .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
        } else {
            None
        };
        let fam_name = resolved_family.or(default_fam.as_deref());
        let Some(fam) = fam_name else {
            return target;
        };
        let mut best_w: Option<u16> = None;
        let mut best_dist = i32::MAX;
        for face in db.faces() {
            if face.style != Style::Normal {
                continue;
            }
            if !face
                .families
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case(fam))
            {
                continue;
            }
            let fw = face.weight.0;
            let dist = (i32::from(fw) - i32::from(target)).abs();
            if dist < best_dist || (dist == best_dist && best_w.is_some_and(|bw| fw > bw)) {
                best_dist = dist;
                best_w = Some(fw);
            }
        }
        best_w.unwrap_or(target)
    }

    /// Returns `(width, height)` of the RGBA8 atlas texture in pixels.
    pub fn atlas_size(&self) -> [u32; 2] {
        [self.atlas_w, self.atlas_h]
    }

    /// Returns the fraction (`0.0..=1.0`) of atlas area currently occupied by cached glyphs.
    pub fn occupancy_ratio(&self) -> f32 {
        let total = f64::from(self.atlas_w) * f64::from(self.atlas_h);
        if total <= 0.0 {
            0.0
        } else {
            (self.used_area as f64 / total) as f32
        }
    }

    /// Returns the atlas RGBA8 bytes if modified since the last upload and clears the dirty flag.
    #[cfg(test)]
    pub fn take_dirty_upload(&mut self) -> Option<(u32, u32, &[u8])> {
        if self.dirty_upload {
            self.dirty_upload = false;
            self.dirty_full = false;
            self.dirty_y_range = None;
            Some((self.atlas_w, self.atlas_h, &self.pixels))
        } else {
            None
        }
    }

    /// Returns either a full atlas upload (on init/resize) or only the dirty row band for `tex_sub_image_2d`.
    pub fn take_dirty_upload_region(&mut self) -> Option<AtlasUpload<'_>> {
        if !self.dirty_upload {
            return None;
        }
        self.dirty_upload = false;
        if self.dirty_full {
            self.dirty_full = false;
            self.dirty_y_range = None;
            return Some(AtlasUpload::Full {
                width: self.atlas_w,
                height: self.atlas_h,
                pixels: &self.pixels,
            });
        }
        if let Some((y0, y1)) = self.dirty_y_range.take() {
            let y0 = y0.min(self.atlas_h);
            let y1 = y1.min(self.atlas_h);
            if y1 > y0 {
                let row_bytes = (self.atlas_w as usize).saturating_mul(4);
                let start = (y0 as usize).saturating_mul(row_bytes);
                let end = (y1 as usize).saturating_mul(row_bytes);
                if end <= self.pixels.len() {
                    return Some(AtlasUpload::SubRows {
                        atlas_width: self.atlas_w,
                        atlas_height: self.atlas_h,
                        y: y0,
                        height: y1 - y0,
                        pixels: &self.pixels[start..end],
                    });
                }
            }
        }
        Some(AtlasUpload::Full {
            width: self.atlas_w,
            height: self.atlas_h,
            pixels: &self.pixels,
        })
    }

    /// Returns the raw CPU RGBA8 pixel slice of the atlas.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Converts integer atlas texel coordinates `[x, y, w, h]` into normalized `[u0, v0, u1, v1]`.
    pub fn normalized_uv(&self, px_rect: [u32; 4]) -> [f32; 4] {
        let aw = self.atlas_w as f32;
        let ah = self.atlas_h as f32;
        [
            px_rect[0] as f32 / aw,
            px_rect[1] as f32 / ah,
            (px_rect[0] + px_rect[2]) as f32 / aw,
            (px_rect[1] + px_rect[3]) as f32 / ah,
        ]
    }

    fn grow_atlas(&mut self) -> bool {
        let Some(new_w) = self.atlas_w.checked_mul(2) else {
            return false;
        };
        let Some(new_h) = self.atlas_h.checked_mul(2) else {
            return false;
        };
        if new_w > MAX_ATLAS_SIZE || new_h > MAX_ATLAS_SIZE {
            return false;
        }
        let Some(new_len) = usize::try_from(new_w)
            .ok()
            .and_then(|w| usize::try_from(new_h).ok().and_then(|h| w.checked_mul(h)))
            .and_then(|px| px.checked_mul(4))
        else {
            return false;
        };

        let mut next = vec![0u8; new_len];
        let old_row_bytes = (self.atlas_w as usize) * 4;
        let new_row_bytes = (new_w as usize) * 4;
        for row in 0..(self.atlas_h as usize) {
            let src_start = row * old_row_bytes;
            let dst_start = row * new_row_bytes;
            next[dst_start..dst_start + old_row_bytes]
                .copy_from_slice(&self.pixels[src_start..src_start + old_row_bytes]);
        }

        self.atlas_w = new_w;
        self.atlas_h = new_h;
        self.pixels = next;
        self.dirty_upload = true;
        self.dirty_full = true;
        self.dirty_y_range = None;
        true
    }

    /// Clears all packed glyphs from the atlas while preserving allocated texture dimensions,
    /// forcing a full GPU upload on the next frame so active glyphs can be freshly repacked.
    pub fn reset_atlas(&mut self) {
        self.pixels.fill(0);
        for y in 0..2u32 {
            for x in 0..2u32 {
                let idx = ((y as usize) * (self.atlas_w as usize) + (x as usize)) * 4;
                self.pixels[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        self.cursor_x = 3;
        self.cursor_y = 1;
        self.shelf_h = 2;
        self.used_area = 4;
        self.cache.clear();
        self.shape_cache.clear();
        self.dirty_upload = true;
        self.dirty_full = true;
        self.dirty_y_range = None;
        self.atlas_epoch = self.atlas_epoch.wrapping_add(1);
    }

    pub(crate) fn begin_batch(&mut self) {
        self.batch_defer_reset = true;
        self.batch_overflowed = false;
    }

    pub(crate) fn end_batch(&mut self) -> bool {
        self.batch_defer_reset = false;
        let overflowed = self.batch_overflowed;
        self.batch_overflowed = false;
        overflowed
    }

    fn alloc_atlas_slot(&mut self, w: u32, h: u32) -> Option<[u32; 2]> {
        let w_pad = w.checked_add(1)?;
        let h_pad = h.checked_add(1)?;
        if w_pad > MAX_ATLAS_SIZE || h_pad > MAX_ATLAS_SIZE {
            return None;
        }

        if self.cursor_x.checked_add(w_pad)? > self.atlas_w {
            self.cursor_y = self.cursor_y.checked_add(self.shelf_h.checked_add(1)?)?;
            self.cursor_x = 1;
            self.shelf_h = 0;
        }

        if self.cursor_y.checked_add(h_pad)? > self.atlas_h {
            if !self.grow_atlas() {
                return None;
            }
            return self.alloc_atlas_slot(w, h);
        }

        let pos = [self.cursor_x, self.cursor_y];
        self.cursor_x = self.cursor_x.checked_add(w_pad)?;
        if h > self.shelf_h {
            self.shelf_h = h;
        }
        self.used_area = self
            .used_area
            .saturating_add(u64::from(w).saturating_mul(u64::from(h)));
        Some(pos)
    }

    fn ensure_glyph(&mut self, cache_key: CacheKey) -> GlyphLookup {
        if let Some(entry) = self.cache.get(&cache_key) {
            return match *entry {
                Some(cached) => GlyphLookup::Ready(cached),
                None => GlyphLookup::Empty,
            };
        }

        let img_opt = self
            .swash_cache
            .get_image_uncached(&mut self.font_system, cache_key);
        let Some(img) = img_opt else {
            self.cache.insert(cache_key, None);
            return GlyphLookup::Empty;
        };

        let gw = img.placement.width;
        let gh = img.placement.height;
        if gw == 0 || gh == 0 {
            self.cache.insert(cache_key, None);
            return GlyphLookup::Empty;
        }
        if gw.saturating_add(1) > MAX_ATLAS_SIZE || gh.saturating_add(1) > MAX_ATLAS_SIZE {
            self.cache.insert(cache_key, None);
            return GlyphLookup::Empty;
        }

        let [ax, ay] = match self.alloc_atlas_slot(gw, gh) {
            Some(pos) => pos,
            None => {
                if self.batch_defer_reset {
                    self.batch_overflowed = true;
                    return GlyphLookup::Overflow;
                }
                self.reset_atlas();
                match self.alloc_atlas_slot(gw, gh) {
                    Some(pos) => pos,
                    None => return GlyphLookup::Overflow,
                }
            }
        };

        let is_color = matches!(img.content, SwashContent::Color);
        let aw = self.atlas_w as usize;

        match img.content {
            SwashContent::Mask => {
                for row in 0..gh {
                    for col in 0..gw {
                        let alpha = img.data[(row * gw + col) as usize];
                        let dst = ((ay + row) as usize * aw + (ax + col) as usize) * 4;
                        self.pixels[dst..dst + 4].copy_from_slice(&[255, 255, 255, alpha]);
                    }
                }
            }
            SwashContent::Color => {
                for row in 0..gh {
                    for col in 0..gw {
                        let src = ((row * gw + col) as usize) * 4;
                        let dst = ((ay + row) as usize * aw + (ax + col) as usize) * 4;
                        self.pixels[dst..dst + 4].copy_from_slice(&img.data[src..src + 4]);
                    }
                }
            }
            SwashContent::SubpixelMask => {
                for row in 0..gh {
                    for col in 0..gw {
                        let src = ((row * gw + col) as usize) * 3;
                        let alpha = img.data.get(src).copied().unwrap_or(0);
                        let dst = ((ay + row) as usize * aw + (ax + col) as usize) * 4;
                        self.pixels[dst..dst + 4].copy_from_slice(&[255, 255, 255, alpha]);
                    }
                }
            }
        }

        self.dirty_upload = true;
        if !self.dirty_full {
            let y0 = ay;
            let y1 = ay.saturating_add(gh).min(self.atlas_h);
            self.dirty_y_range = Some(match self.dirty_y_range {
                Some((prev_y0, prev_y1)) => (prev_y0.min(y0), prev_y1.max(y1)),
                None => (y0, y1),
            });
        }
        let cached = CachedGlyph {
            px_x: ax,
            px_y: ay,
            px_w: gw,
            px_h: gh,
            left: img.placement.left,
            top: img.placement.top,
            is_color,
        };
        self.cache.insert(cache_key, Some(cached));
        GlyphLookup::Ready(cached)
    }

    fn attrs_for_span<'a>(
        kind: SpanKind,
        idx: usize,
        has_italic_face: bool,
        custom_family: Option<&'a str>,
        mono_family: Option<&'a str>,
    ) -> Attrs<'a> {
        let mut base = Attrs::new().metadata(idx);
        if let Some(fam) = custom_family {
            if !matches!(kind, SpanKind::Code | SpanKind::Mono) {
                base = base.family(Family::Name(fam));
            }
        }
        match kind {
            SpanKind::Text | SpanKind::Link => base,
            SpanKind::Bold => base.weight(Weight::BOLD),
            SpanKind::Italic => {
                if has_italic_face {
                    base.style(Style::Italic)
                } else {
                    base.cache_key_flags(CacheKeyFlags::FAKE_ITALIC)
                }
            }
            SpanKind::Code | SpanKind::Mono => {
                if let Some(mf) = mono_family {
                    base.family(Family::Name(mf))
                } else {
                    base.family(Family::Monospace)
                }
            }
            SpanKind::Mention => base.weight(Weight::SEMIBOLD),
        }
    }

    fn measure_buffer(
        buffer: &Buffer,
        font_size: f32,
        tracking: f32,
        line_height_mult: f32,
    ) -> [f32; 2] {
        let lh_mult = if line_height_mult.is_finite() && line_height_mult > 0.0 {
            line_height_mult
        } else {
            1.35
        };
        let track_px = if tracking.is_finite() {
            tracking * font_size
        } else {
            0.0
        };
        let mut max_w = 0.0_f32;
        let mut total_h = 0.0_f32;
        for run in buffer.layout_runs() {
            let extra = if run.glyphs.len() > 1 {
                (run.glyphs.len() - 1) as f32 * track_px
            } else {
                0.0
            };
            max_w = max_w.max((run.line_w + extra).max(0.0));
            total_h = total_h.max(run.line_top + run.line_height);
        }
        if total_h <= 0.0 {
            total_h = font_size * lh_mult;
        }
        let w = if max_w > 0.0 { max_w.ceil() + 3.0 } else { 0.0 };
        [w, total_h.ceil()]
    }

    #[allow(clippy::too_many_arguments)]
    fn truncate_with_ellipsis(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        mono: bool,
        font_family: Option<&str>,
        tracking: f32,
        line_height: f32,
        max_width: f32,
    ) -> String {
        let lh_mult = if line_height.is_finite() && line_height > 0.0 {
            line_height
        } else {
            1.35
        };
        let metrics = Metrics::new(font_size.max(1.0), (font_size * lh_mult).max(1.0));
        let resolved_family = self.effective_family_owned(font_family, mono);
        let effective_weight =
            self.resolve_weight_for_family(resolved_family.as_deref(), mono, weight);
        let mut attrs = Attrs::new().weight(Weight(effective_weight));
        if let Some(ref fam) = resolved_family {
            attrs = attrs.family(Family::Name(fam.as_str()));
        } else if mono {
            attrs = attrs.family(Family::Monospace);
        }
        self.scratch_buf.set_metrics(&mut self.font_system, metrics);
        {
            let mut b = self.scratch_buf.borrow_with(&mut self.font_system);
            b.set_wrap(Wrap::None);
            b.set_size(None, None);
            b.set_text(text, attrs, Shaping::Advanced);
            b.shape_until_scroll(true);
        }
        let full_w = Self::measure_buffer(&self.scratch_buf, font_size, tracking, lh_mult)[0];
        if full_w <= max_width {
            return String::from(text);
        }

        let mut boundaries: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        boundaries.push(text.len());
        let mut lo = 0usize;
        let mut hi = boundaries.len().saturating_sub(1);
        let mut best = 0usize;
        let mut candidate = String::with_capacity(text.len().min(64) + 3);
        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            let cut = boundaries[mid];
            candidate.clear();
            candidate.push_str(&text[..cut]);
            candidate.push_str("...");
            {
                let mut b = self.scratch_buf.borrow_with(&mut self.font_system);
                b.set_wrap(Wrap::None);
                b.set_size(None, None);
                b.set_text(&candidate, attrs, Shaping::Advanced);
                b.shape_until_scroll(true);
            }
            if Self::measure_buffer(&self.scratch_buf, font_size, tracking, lh_mult)[0]
                <= max_width
            {
                best = cut;
                lo = mid + 1;
            } else if mid == 0 {
                break;
            } else {
                hi = mid - 1;
            }
        }
        candidate.clear();
        candidate.push_str(&text[..best]);
        candidate.push_str("...");
        candidate
    }

    /// Shapes plain text into physical-pixel glyph quads inside `rect`.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn shape_plain_to_quads(
        &mut self,
        text: &str,
        rect: Rect,
        scale_factor: f32,
        color: Color,
        font_size: f32,
        wrap: bool,
        ellipsis: bool,
    ) -> ShapedTextOutput {
        self.shape_plain_formatted_to_quads(
            text,
            rect,
            scale_factor,
            color,
            font_size,
            400,
            false,
            None,
            wrap,
            ellipsis,
            TextAlign::Left,
            0.0,
            1.35,
        )
    }

    /// Shapes plain text with explicit weight, monospace, custom font family, alignment, tracking (em), and line height multiplier.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_plain_formatted_to_quads(
        &mut self,
        text: &str,
        rect: Rect,
        scale_factor: f32,
        color: Color,
        font_size: f32,
        weight: u16,
        mono: bool,
        font_family: Option<&str>,
        wrap: bool,
        ellipsis: bool,
        align: TextAlign,
        tracking: f32,
        line_height: f32,
    ) -> ShapedTextOutput {
        self.sync_registered_fonts();
        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let lh_mult = if line_height.is_finite() && line_height > 0.0 {
            line_height
        } else {
            1.35
        };
        let track_px = if tracking.is_finite() {
            tracking * font_size * scale
        } else {
            0.0
        };
        let origin_x_px = (rect.x * scale).round();
        let origin_y_px = (rect.y * scale).round();
        let rect_w_px = (rect.w * scale).max(0.0);

        let mut key_h = DualHash::new();
        key_h.write_u32(1);
        key_h.write_bytes(text.as_bytes());
        key_h.write_u32(scale.to_bits());
        key_h.write_u32(font_size.to_bits());
        key_h.write_u32(u32::from(weight.clamp(100, 900)));
        key_h.write_u32(
            u32::from(mono)
                | (u32::from(wrap) << 1)
                | (u32::from(ellipsis) << 2)
                | ((align as u32) << 3),
        );
        if let Some(fam) = font_family {
            key_h.write_bytes(fam.as_bytes());
        }
        key_h.write_u32(track_px.to_bits());
        key_h.write_u32(lh_mult.to_bits());
        if wrap || ellipsis || align != TextAlign::Left {
            key_h.write_u32(rect.w.to_bits());
        }
        if rect.h > 0.0 {
            key_h.write_u32(rect.h.to_bits());
        }
        let cache_key = key_h.finish();

        if let Some(cached_glyphs) = self.shape_cache.get(&cache_key) {
            let mut out = ShapedTextOutput {
                bg_rects: Vec::new(),
                glyphs: Vec::with_capacity(cached_glyphs.len()),
                fg_rects: Vec::new(),
            };
            for g in cached_glyphs {
                let qcolor = if g.is_color {
                    Color::rgba(255, 255, 255, color.a)
                } else {
                    color
                };
                out.glyphs.push(ShapedGlyphQuad {
                    phys_rect: [origin_x_px + g.rel_x, origin_y_px + g.rel_y, g.w, g.h],
                    atlas_px: g.atlas_px,
                    color: qcolor,
                });
            }
            return out;
        }

        let display_text = if ellipsis && !wrap && rect.w > 0.0 {
            self.truncate_with_ellipsis(
                text,
                font_size,
                weight,
                mono,
                font_family,
                tracking,
                lh_mult,
                rect.w,
            )
        } else {
            String::from(text)
        };

        let metrics = Metrics::new(font_size.max(1.0), (font_size * lh_mult).max(1.0));
        let resolved_family = self.effective_family_owned(font_family, mono);
        let effective_weight =
            self.resolve_weight_for_family(resolved_family.as_deref(), mono, weight);
        let mut attrs = Attrs::new().weight(Weight(effective_weight));
        if let Some(ref fam) = resolved_family {
            attrs = attrs.family(Family::Name(fam.as_str()));
        } else if mono {
            attrs = attrs.family(Family::Monospace);
        }
        let mut buf = core::mem::replace(&mut self.scratch_buf, Buffer::new_empty(metrics));
        buf.set_metrics(&mut self.font_system, metrics);
        {
            let mut b = buf.borrow_with(&mut self.font_system);
            b.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
            let max_w = if wrap && rect.w > 0.0 {
                Some(rect.w)
            } else {
                None
            };
            b.set_size(max_w, None);
            b.set_text(&display_text, attrs, Shaping::Advanced);
            b.shape_until_scroll(true);
        }

        let mut out = ShapedTextOutput::default();
        let mut cached_list = Vec::new();
        let mut had_overflow = false;

        for attempt in 0..2 {
            let start_epoch = self.atlas_epoch;
            out.glyphs.clear();
            cached_list.clear();
            had_overflow = false;

            for run in buf.layout_runs() {
                let line_y_px = (run.line_y * scale).round();
                if rect.h > 0.0 && run.line_top > rect.h {
                    break;
                }
                let extra_track = if run.glyphs.len() > 1 {
                    (run.glyphs.len() - 1) as f32 * track_px
                } else {
                    0.0
                };
                let line_w_px = (run.line_w * scale + extra_track).max(0.0);
                let align_x_px = match align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => ((rect_w_px - line_w_px).max(0.0) * 0.5).round(),
                    TextAlign::Right => (rect_w_px - line_w_px).max(0.0).round(),
                };
                for (gi, glyph) in run.glyphs.iter().enumerate() {
                    let phys = glyph.physical((0.0, 0.0), scale);
                    let cached = match self.ensure_glyph(phys.cache_key) {
                        GlyphLookup::Ready(c) => c,
                        GlyphLookup::Empty => continue,
                        GlyphLookup::Overflow => {
                            had_overflow = true;
                            continue;
                        }
                    };
                    let rel_x = align_x_px
                        + phys.x as f32
                        + (gi as f32 * track_px).round()
                        + cached.left as f32;
                    let rel_y = line_y_px + phys.y as f32 - cached.top as f32;
                    let gx = origin_x_px + rel_x;
                    let gy = origin_y_px + rel_y;
                    let qcolor = if cached.is_color {
                        Color::rgba(255, 255, 255, color.a)
                    } else {
                        color
                    };
                    let atlas_px = [cached.px_x, cached.px_y, cached.px_w, cached.px_h];
                    cached_list.push(CachedShapedGlyph {
                        rel_x,
                        rel_y,
                        w: cached.px_w as f32,
                        h: cached.px_h as f32,
                        atlas_px,
                        is_color: cached.is_color,
                    });
                    out.glyphs.push(ShapedGlyphQuad {
                        phys_rect: [gx, gy, cached.px_w as f32, cached.px_h as f32],
                        atlas_px,
                        color: qcolor,
                    });
                }
            }

            if self.atlas_epoch != start_epoch && attempt == 0 {
                continue;
            }
            break;
        }
        self.scratch_buf = buf;
        if !had_overflow {
            if self.shape_cache.len() >= MAX_TEXT_CACHE_ENTRIES {
                self.shape_cache.clear();
            }
            self.shape_cache.insert(cache_key, cached_list);
        }
        out
    }

    /// Shapes rich-text `OwnedSpan` runs into glyph quads, pill backgrounds, and link underlines.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn shape_rich_to_quads(
        &mut self,
        spans: &[OwnedSpan],
        rect: Rect,
        scale_factor: f32,
        default_color: Color,
        code_bg: Color,
        mention_bg: Color,
        link_color: Color,
        font_size: f32,
        wrap: bool,
        ellipsis: bool,
    ) -> ShapedTextOutput {
        self.shape_rich_formatted_to_quads(
            spans,
            rect,
            scale_factor,
            default_color,
            code_bg,
            mention_bg,
            link_color,
            font_size,
            None,
            wrap,
            ellipsis,
            TextAlign::Left,
            0.0,
            1.35,
        )
    }

    /// Shapes rich-text `OwnedSpan` runs with explicit font family, alignment, tracking (em), and line height multiplier.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_rich_formatted_to_quads(
        &mut self,
        spans: &[OwnedSpan],
        rect: Rect,
        scale_factor: f32,
        default_color: Color,
        code_bg: Color,
        mention_bg: Color,
        link_color: Color,
        font_size: f32,
        font_family: Option<&str>,
        wrap: bool,
        ellipsis: bool,
        align: TextAlign,
        tracking: f32,
        line_height: f32,
    ) -> ShapedTextOutput {
        self.sync_registered_fonts();
        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let lh_mult = if line_height.is_finite() && line_height > 0.0 {
            line_height
        } else {
            1.35
        };
        let track_px = if tracking.is_finite() {
            tracking * font_size * scale
        } else {
            0.0
        };

        if ellipsis && !wrap && rect.w > 0.0 {
            let combined: String = spans.iter().map(|s| s.text.as_str()).collect();
            let truncated = self.truncate_with_ellipsis(
                &combined,
                font_size,
                400,
                false,
                font_family,
                tracking,
                lh_mult,
                rect.w,
            );
            if truncated.len() < combined.len() {
                return self.shape_plain_formatted_to_quads(
                    &truncated,
                    rect,
                    scale,
                    default_color,
                    font_size,
                    400,
                    false,
                    font_family,
                    false,
                    false,
                    align,
                    tracking,
                    lh_mult,
                );
            }
        }

        let metrics = Metrics::new(font_size.max(1.0), (font_size * lh_mult).max(1.0));
        let has_italic_face = self.has_italic_face;
        let resolved_family = self.effective_family_owned(font_family, false);
        let mono_family = self.applied_default_mono.clone();
        let mut buf = core::mem::replace(&mut self.scratch_buf, Buffer::new_empty(metrics));
        buf.set_metrics(&mut self.font_system, metrics);
        {
            let mut b = buf.borrow_with(&mut self.font_system);
            b.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
            let max_w = if wrap && rect.w > 0.0 {
                Some(rect.w)
            } else {
                None
            };
            b.set_size(max_w, None);
            let default_attrs = Attrs::new();
            let fam_ref = resolved_family.as_deref();
            let mono_ref = mono_family.as_deref();
            let rich_iter = spans.iter().enumerate().map(|(i, sp)| {
                (
                    sp.text.as_str(),
                    Self::attrs_for_span(sp.kind, i, has_italic_face, fam_ref, mono_ref),
                )
            });
            b.set_rich_text(rich_iter, default_attrs, Shaping::Advanced);
            b.shape_until_scroll(true);
        }

        let origin_x_px = (rect.x * scale).round();
        let origin_y_px = (rect.y * scale).round();
        let rect_w_px = (rect.w * scale).max(0.0);
        let mut out = ShapedTextOutput::default();

        for attempt in 0..2 {
            let start_epoch = self.atlas_epoch;
            out.bg_rects.clear();
            out.glyphs.clear();
            out.fg_rects.clear();

            for run in buf.layout_runs() {
                let line_y_px = (run.line_y * scale).round();
                let line_top_px = origin_y_px + (run.line_top * scale).round();
                let line_h_px = (run.line_height * scale).round();
                if rect.h > 0.0 && run.line_top > rect.h {
                    break;
                }
                let extra_track = if run.glyphs.len() > 1 {
                    (run.glyphs.len() - 1) as f32 * track_px
                } else {
                    0.0
                };
                let line_w_px = (run.line_w * scale + extra_track).max(0.0);
                let align_x_px = match align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => ((rect_w_px - line_w_px).max(0.0) * 0.5).round(),
                    TextAlign::Right => (rect_w_px - line_w_px).max(0.0).round(),
                };

                let mut cur_span_idx: Option<usize> = None;
                let mut span_x0 = 0.0_f32;
                let mut span_x1 = 0.0_f32;

                let flush_span_decor = |idx_opt: Option<usize>,
                                        x0: f32,
                                        x1: f32,
                                        out: &mut ShapedTextOutput| {
                    let Some(si) = idx_opt else {
                        return;
                    };
                    let Some(sp) = spans.get(si) else {
                        return;
                    };
                    let w = (x1 - x0).max(0.0);
                    if w <= 0.0 {
                        return;
                    }
                    match sp.kind {
                        SpanKind::Code => {
                            out.bg_rects.push(RichDecorationRect {
                                phys_rect: [
                                    x0 - 2.0 * scale,
                                    line_top_px,
                                    w + 4.0 * scale,
                                    line_h_px,
                                ],
                                color: code_bg,
                                radius: 4.0 * scale,
                            });
                        }
                        SpanKind::Mention => {
                            out.bg_rects.push(RichDecorationRect {
                                phys_rect: [
                                    x0 - 2.0 * scale,
                                    line_top_px,
                                    w + 4.0 * scale,
                                    line_h_px,
                                ],
                                color: mention_bg,
                                radius: 4.0 * scale,
                            });
                        }
                        SpanKind::Link => {
                            out.fg_rects.push(RichDecorationRect {
                                phys_rect: [
                                    x0,
                                    origin_y_px + line_y_px + (2.0 * scale).round(),
                                    w,
                                    (1.0 * scale).max(1.0),
                                ],
                                color: link_color,
                                radius: 0.0,
                            });
                        }
                        _ => {}
                    }
                };

                for (gi, glyph) in run.glyphs.iter().enumerate() {
                    let t_off = (gi as f32 * track_px).round();
                    let gx0 = origin_x_px + align_x_px + (glyph.x * scale).round() + t_off;
                    let gx1 =
                        origin_x_px + align_x_px + ((glyph.x + glyph.w) * scale).round() + t_off;

                    if cur_span_idx != Some(glyph.metadata) {
                        flush_span_decor(cur_span_idx, span_x0, span_x1, &mut out);
                        cur_span_idx = Some(glyph.metadata);
                        span_x0 = gx0;
                        span_x1 = gx1;
                    } else {
                        span_x0 = span_x0.min(gx0);
                        span_x1 = span_x1.max(gx1);
                    }

                    let phys = glyph.physical((0.0, 0.0), scale);
                    let cached = match self.ensure_glyph(phys.cache_key) {
                        GlyphLookup::Ready(c) => c,
                        GlyphLookup::Empty | GlyphLookup::Overflow => continue,
                    };

                    let base_col = if let Some(c) = spans.get(glyph.metadata).and_then(|s| s.color)
                    {
                        c
                    } else {
                        match spans.get(glyph.metadata).map(|s| s.kind) {
                            Some(SpanKind::Link) | Some(SpanKind::Mention) => link_color,
                            _ => default_color,
                        }
                    };
                    let qcolor = if cached.is_color {
                        Color::rgba(255, 255, 255, base_col.a)
                    } else {
                        base_col
                    };

                    let qx = origin_x_px + align_x_px + phys.x as f32 + t_off + cached.left as f32;
                    let qy = origin_y_px + line_y_px + phys.y as f32 - cached.top as f32;
                    out.glyphs.push(ShapedGlyphQuad {
                        phys_rect: [qx, qy, cached.px_w as f32, cached.px_h as f32],
                        atlas_px: [cached.px_x, cached.px_y, cached.px_w, cached.px_h],
                        color: qcolor,
                    });
                }

                flush_span_decor(cur_span_idx, span_x0, span_x1, &mut out);
            }

            if self.atlas_epoch != start_epoch && attempt == 0 {
                continue;
            }
            break;
        }

        self.scratch_buf = buf;
        out
    }
}

impl TextMeasure for GlyphAtlas {
    fn measure_plain(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        self.measure_plain_weighted(text, font_size, 400, max_width, wrap, ellipsis)
    }

    fn measure_plain_weighted(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        self.measure_plain_styled(text, font_size, weight, false, max_width, wrap, ellipsis)
    }

    fn measure_plain_styled(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        mono: bool,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        self.measure_plain_formatted(
            text, font_size, weight, mono, None, 0.0, 1.35, max_width, wrap, ellipsis,
        )
    }

    fn measure_plain_formatted(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        mono: bool,
        font_family: Option<&str>,
        tracking: f32,
        line_height: f32,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        self.sync_registered_fonts();
        let lh_mult = if line_height.is_finite() && line_height > 0.0 {
            line_height
        } else {
            1.35
        };
        let mut key_h = DualHash::new();
        key_h.write_u32(2);
        key_h.write_bytes(text.as_bytes());
        key_h.write_u32(font_size.to_bits());
        key_h.write_u32(u32::from(weight.clamp(100, 900)));
        key_h.write_u32(u32::from(mono) | (u32::from(wrap) << 1) | (u32::from(ellipsis) << 2));
        if let Some(fam) = font_family {
            key_h.write_bytes(fam.as_bytes());
        }
        key_h.write_u32(tracking.to_bits());
        key_h.write_u32(lh_mult.to_bits());
        if let Some(mw) = max_width {
            key_h.write_u32(1);
            key_h.write_u32(mw.to_bits());
        } else {
            key_h.write_u32(0);
        }
        let cache_key = key_h.finish();
        if let Some(&cached) = self.measure_cache.get(&cache_key) {
            return cached;
        }

        let metrics = Metrics::new(font_size.max(1.0), (font_size * lh_mult).max(1.0));
        let resolved_family = self.effective_family_owned(font_family, mono);
        let effective_weight =
            self.resolve_weight_for_family(resolved_family.as_deref(), mono, weight);
        let mut attrs = Attrs::new().weight(Weight(effective_weight));
        if let Some(ref fam) = resolved_family {
            attrs = attrs.family(Family::Name(fam.as_str()));
        } else if mono {
            attrs = attrs.family(Family::Monospace);
        }
        let mut buf = core::mem::replace(&mut self.scratch_buf, Buffer::new_empty(metrics));
        buf.set_metrics(&mut self.font_system, metrics);
        {
            let mut b = buf.borrow_with(&mut self.font_system);
            b.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
            b.set_size(if wrap { max_width } else { None }, None);
            b.set_text(text, attrs, Shaping::Advanced);
            b.shape_until_scroll(true);
        }
        let mut sz = Self::measure_buffer(&buf, font_size, tracking, lh_mult);
        self.scratch_buf = buf;
        if wrap || ellipsis {
            if let Some(mw) = max_width {
                sz[0] = sz[0].min(mw);
            }
        }
        if self.measure_cache.len() >= MAX_TEXT_CACHE_ENTRIES {
            self.measure_cache.clear();
        }
        self.measure_cache.insert(cache_key, sz);
        sz
    }

    fn measure_rich(
        &mut self,
        spans: &[OwnedSpan],
        font_size: f32,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        self.measure_rich_formatted(spans, font_size, None, 0.0, 1.35, max_width, wrap, ellipsis)
    }

    fn measure_rich_formatted(
        &mut self,
        spans: &[OwnedSpan],
        font_size: f32,
        font_family: Option<&str>,
        tracking: f32,
        line_height: f32,
        max_width: Option<f32>,
        wrap: bool,
        ellipsis: bool,
    ) -> [f32; 2] {
        self.sync_registered_fonts();
        let lh_mult = if line_height.is_finite() && line_height > 0.0 {
            line_height
        } else {
            1.35
        };
        let mut key_h = DualHash::new();
        key_h.write_u32(3);
        key_h.write_u32(spans.len() as u32);
        for sp in spans {
            key_h.write_u32(sp.kind as u32);
            key_h.write_bytes(sp.text.as_bytes());
        }
        key_h.write_u32(font_size.to_bits());
        key_h.write_u32(u32::from(wrap) | (u32::from(ellipsis) << 1));
        if let Some(fam) = font_family {
            key_h.write_bytes(fam.as_bytes());
        }
        key_h.write_u32(tracking.to_bits());
        key_h.write_u32(lh_mult.to_bits());
        if let Some(mw) = max_width {
            key_h.write_u32(1);
            key_h.write_u32(mw.to_bits());
        } else {
            key_h.write_u32(0);
        }
        let cache_key = key_h.finish();
        if let Some(&cached) = self.measure_cache.get(&cache_key) {
            return cached;
        }

        let metrics = Metrics::new(font_size.max(1.0), (font_size * lh_mult).max(1.0));
        let has_italic_face = self.has_italic_face;
        let resolved_family = self.effective_family_owned(font_family, false);
        let mono_family = self.applied_default_mono.clone();
        let mut buf = core::mem::replace(&mut self.scratch_buf, Buffer::new_empty(metrics));
        buf.set_metrics(&mut self.font_system, metrics);
        {
            let mut b = buf.borrow_with(&mut self.font_system);
            b.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
            b.set_size(if wrap { max_width } else { None }, None);
            let default_attrs = Attrs::new();
            let fam_ref = resolved_family.as_deref();
            let mono_ref = mono_family.as_deref();
            let rich_iter = spans.iter().enumerate().map(|(i, sp)| {
                (
                    sp.text.as_str(),
                    Self::attrs_for_span(sp.kind, i, has_italic_face, fam_ref, mono_ref),
                )
            });
            b.set_rich_text(rich_iter, default_attrs, Shaping::Advanced);
            b.shape_until_scroll(true);
        }
        let mut sz = Self::measure_buffer(&buf, font_size, tracking, lh_mult);
        self.scratch_buf = buf;
        if wrap || ellipsis {
            if let Some(mw) = max_width {
                sz[0] = sz[0].min(mw);
            }
        }
        if self.measure_cache.len() >= MAX_TEXT_CACHE_ENTRIES {
            self.measure_cache.clear();
        }
        self.measure_cache.insert(cache_key, sz);
        sz
    }

    fn measure_rich_link_rects(
        &mut self,
        spans: &[OwnedSpan],
        rect: Rect,
        font_size: f32,
        font_family: Option<&str>,
        wrap: bool,
        align: TextAlign,
        tracking: f32,
        line_height: f32,
    ) -> Vec<(usize, Rect)> {
        self.sync_registered_fonts();
        let lh_mult = if line_height.is_finite() && line_height > 0.0 {
            line_height
        } else {
            1.35
        };
        let track_px = if tracking.is_finite() {
            tracking * font_size
        } else {
            0.0
        };
        let metrics = Metrics::new(font_size.max(1.0), (font_size * lh_mult).max(1.0));
        let has_italic_face = self.has_italic_face;
        let resolved_family = self.effective_family_owned(font_family, false);
        let mono_family = self.applied_default_mono.clone();
        let mut buf = core::mem::replace(&mut self.scratch_buf, Buffer::new_empty(metrics));
        buf.set_metrics(&mut self.font_system, metrics);
        {
            let mut b = buf.borrow_with(&mut self.font_system);
            b.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
            let max_w = if wrap && rect.w > 0.0 {
                Some(rect.w)
            } else {
                None
            };
            b.set_size(max_w, None);
            let default_attrs = Attrs::new();
            let fam_ref = resolved_family.as_deref();
            let mono_ref = mono_family.as_deref();
            let rich_iter = spans.iter().enumerate().map(|(i, sp)| {
                (
                    sp.text.as_str(),
                    Self::attrs_for_span(sp.kind, i, has_italic_face, fam_ref, mono_ref),
                )
            });
            b.set_rich_text(rich_iter, default_attrs, Shaping::Advanced);
            b.shape_until_scroll(true);
        }

        let mut out = Vec::new();
        for run in buf.layout_runs() {
            let extra_track = if run.glyphs.len() > 1 {
                (run.glyphs.len() - 1) as f32 * track_px
            } else {
                0.0
            };
            let line_w = (run.line_w + extra_track).max(0.0);
            let align_x = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (rect.w - line_w).max(0.0) * 0.5,
                TextAlign::Right => (rect.w - line_w).max(0.0),
            };
            let mut cur_span: Option<usize> = None;
            let mut sx0 = 0.0_f32;
            let mut sx1 = 0.0_f32;
            let line_y = rect.y + run.line_top;
            let line_h = run.line_height.max(font_size);

            for (gi, g) in run.glyphs.iter().enumerate() {
                let t_off = gi as f32 * track_px;
                let gx0 = rect.x + align_x + g.x + t_off;
                let gx1 = rect.x + align_x + g.x + g.w + t_off;
                if cur_span != Some(g.metadata) {
                    if let Some(si) = cur_span {
                        if spans.get(si).is_some_and(|s| s.kind == SpanKind::Link) {
                            out.push((si, Rect::new(sx0, line_y, (sx1 - sx0).max(8.0), line_h)));
                        }
                    }
                    cur_span = Some(g.metadata);
                    sx0 = gx0;
                    sx1 = gx1;
                } else {
                    sx0 = sx0.min(gx0);
                    sx1 = sx1.max(gx1);
                }
            }
            if let Some(si) = cur_span {
                if spans.get(si).is_some_and(|s| s.kind == SpanKind::Link) {
                    out.push((si, Rect::new(sx0, line_y, (sx1 - sx0).max(8.0), line_h)));
                }
            }
        }
        self.scratch_buf = buf;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{GlyphAtlas, FALLBACK_FONT_BYTES};
    use crate::ui::context::TextMeasure;
    use crate::ui::draw::{OwnedSpan, SpanKind};
    use crate::ui::types::{Color, Rect};

    #[test]
    fn fallback_font_under_45kb_shapes_and_rasterizes() {
        assert!(FALLBACK_FONT_BYTES.len() < 45 * 1024);

        let mut atlas = GlyphAtlas::new_fallback_only();
        let sz = atlas.measure_plain("Hello Lumenic 123!", 16.0, None, false, false);
        assert!(sz[0] > 40.0);
        assert!(sz[1] >= 16.0);

        let out = atlas.shape_plain_to_quads(
            "Hello Lumenic 123!",
            Rect::new(10.0, 10.0, 400.0, 40.0),
            1.25,
            Color::WHITE,
            16.0,
            false,
            false,
        );
        assert!(out.glyphs.len() >= 15);
        assert!(atlas.occupancy_ratio() > 0.0);
        assert!(atlas.take_dirty_upload().is_some());
    }

    #[test]
    fn rich_text_word_wrap_and_ellipsis() {
        let mut atlas = GlyphAtlas::new_fallback_only();
        let wrapped = atlas.measure_plain(
            "The quick brown fox jumps over the lazy dog",
            14.0,
            Some(90.0),
            true,
            false,
        );
        let single = atlas.measure_plain(
            "The quick brown fox jumps over the lazy dog",
            14.0,
            None,
            false,
            false,
        );
        assert!(wrapped[1] > single[1]);

        let spans = [
            OwnedSpan {
                kind: SpanKind::Bold,
                text: String::from("Bold "),
                link_id: None,
                color: None,
            },
            OwnedSpan {
                kind: SpanKind::Italic,
                text: String::from("Italic "),
                link_id: None,
                color: None,
            },
            OwnedSpan {
                kind: SpanKind::Mention,
                text: String::from("@dev"),
                link_id: None,
                color: None,
            },
            OwnedSpan {
                kind: SpanKind::Code,
                text: String::from("lumenic"),
                link_id: None,
                color: None,
            },
            OwnedSpan {
                kind: SpanKind::Link,
                text: String::from("docs"),
                link_id: Some(String::from("docs")),
                color: None,
            },
            OwnedSpan {
                kind: SpanKind::Mono,
                text: String::from(" fn"),
                link_id: None,
                color: Some(Color::rgba(255, 120, 80, 255)),
            },
        ];
        let rich_out = atlas.shape_rich_to_quads(
            &spans,
            Rect::new(0.0, 0.0, 300.0, 40.0),
            1.0,
            Color::WHITE,
            Color::from_hex(0x222222),
            Color::from_hex(0x333366),
            Color::from_hex(0x66AAFF),
            14.0,
            false,
            false,
        );
        assert_eq!(rich_out.bg_rects.len(), 2);
        assert!(rich_out
            .glyphs
            .iter()
            .any(|g| g.color == Color::rgba(255, 120, 80, 255)));
        assert_eq!(rich_out.fg_rects.len(), 1);
        assert!(!rich_out.glyphs.is_empty());

        let trunc = atlas.shape_plain_to_quads(
            "VeryLongStringThatMustEllipsize",
            Rect::new(0.0, 0.0, 70.0, 30.0),
            1.0,
            Color::WHITE,
            14.0,
            false,
            true,
        );
        assert!(trunc.glyphs.len() < "VeryLongStringThatMustEllipsize".len());
    }

    #[test]
    fn custom_font_registration_resolves_alias_and_shapes() {
        super::register_font("CustomTestFont", FALLBACK_FONT_BYTES);
        let mut atlas = GlyphAtlas::new_fallback_only();
        let out = atlas.shape_plain_formatted_to_quads(
            "Custom Font",
            Rect::new(0.0, 0.0, 240.0, 40.0),
            1.0,
            Color::WHITE,
            16.0,
            500,
            false,
            Some("CustomTestFont"),
            false,
            false,
            crate::ui::types::TextAlign::Left,
            0.0,
            1.35,
        );
        assert!(atlas.font_aliases.contains_key("customtestfont"));
        assert!(!out.glyphs.is_empty());

        // Also verify `register_font_bytes(&[u8])` registers the embedded OpenType family name ("LumenicSans").
        super::register_font_bytes(FALLBACK_FONT_BYTES);
        let mut atlas2 = GlyphAtlas::new_fallback_only();
        let out2 = atlas2.shape_plain_formatted_to_quads(
            "Embedded Family Name",
            Rect::new(0.0, 0.0, 240.0, 40.0),
            1.0,
            Color::WHITE,
            16.0,
            400,
            false,
            Some("LumenicSans"),
            false,
            false,
            crate::ui::types::TextAlign::Left,
            0.0,
            1.35,
        );
        assert!(atlas2.font_aliases.contains_key("lumenicsans"));
        assert!(!out2.glyphs.is_empty());
    }

    #[test]
    fn default_font_family_overrides_sans_and_mono_globally() {
        super::register_font("GlobalSansAlias", FALLBACK_FONT_BYTES);
        super::register_font("GlobalMonoAlias", FALLBACK_FONT_BYTES);
        super::set_default_font_family("GlobalSansAlias");
        super::set_default_mono_font_family("GlobalMonoAlias");

        let mut atlas = GlyphAtlas::new_fallback_only();
        let out = atlas.shape_plain_formatted_to_quads(
            "Default Global Font",
            Rect::new(0.0, 0.0, 260.0, 40.0),
            1.0,
            Color::WHITE,
            14.0,
            400,
            false,
            None,
            false,
            false,
            crate::ui::types::TextAlign::Left,
            0.0,
            1.35,
        );
        assert!(atlas.applied_default_sans.is_some());
        assert!(atlas.applied_default_mono.is_some());
        assert!(!out.glyphs.is_empty());
    }

    #[test]
    fn binary_search_ellipsis_fits_max_width() {
        let mut atlas = GlyphAtlas::new_fallback_only();
        let full = "Lumenic fast customizable UI framework";
        assert_eq!(
            atlas.truncate_with_ellipsis(full, 14.0, 400, false, None, 0.0, 1.35, 1000.0),
            full
        );
        let truncated =
            atlas.truncate_with_ellipsis(full, 14.0, 400, false, None, 0.0, 1.35, 110.0);
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() < full.len());
        let w = atlas.measure_plain(&truncated, 14.0, None, false, false)[0];
        assert!(w <= 110.0);
        assert_eq!(
            atlas.truncate_with_ellipsis(full, 14.0, 400, false, None, 0.0, 1.35, 1.0),
            "..."
        );
    }

    #[test]
    fn semibold_ellipsis_matches_measure_without_truncating_on_missing_weight() {
        let mut atlas = GlyphAtlas::new_with_system_fonts();
        for sample in ["Joe", "LensError", "Doughnut Princess"] {
            for weight in [500u16, 600u16] {
                let measured = atlas.measure_plain_formatted(
                    sample, 12.5, weight, false, None, 0.0, 1.35, None, false, true,
                );
                assert_eq!(
                    atlas.truncate_with_ellipsis(
                        sample,
                        12.5,
                        weight,
                        false,
                        None,
                        0.0,
                        1.35,
                        measured[0],
                    ),
                    sample,
                    "weight {weight} should not truncate {sample:?} against its own measured width {}",
                    measured[0]
                );
            }
        }
    }

    #[test]
    fn text_caches_translate_origin_and_atlas_tracks_dirty_subrows() {
        let mut atlas = GlyphAtlas::new_fallback_only();
        assert!(matches!(
            atlas.take_dirty_upload_region(),
            Some(super::AtlasUpload::Full { .. })
        ));
        assert!(atlas.take_dirty_upload_region().is_none());

        let m1 = atlas.measure_plain("Lumenic Cache", 16.0, None, false, false);
        let m2 = atlas.measure_plain("Lumenic Cache", 16.0, None, false, false);
        assert_eq!(m1, m2);
        assert!(!atlas.measure_cache.is_empty());

        let first = atlas.shape_plain_to_quads(
            "Lumenic Cache",
            Rect::new(10.0, 20.0, 200.0, 32.0),
            1.0,
            Color::WHITE,
            16.0,
            false,
            false,
        );
        assert!(!first.glyphs.is_empty());
        assert!(matches!(
            atlas.take_dirty_upload_region(),
            Some(super::AtlasUpload::SubRows { atlas_height, height, .. }) if height > 0 && height < atlas_height
        ));
        assert!(atlas.take_dirty_upload_region().is_none());

        // Scrolling (origin shift) and color changes hit the shaping cache cleanly.
        let second = atlas.shape_plain_to_quads(
            "Lumenic Cache",
            Rect::new(50.0, 120.0, 200.0, 32.0),
            1.0,
            Color::rgb(255, 100, 50),
            16.0,
            false,
            false,
        );
        assert_eq!(first.glyphs.len(), second.glyphs.len());
        assert!(
            (second.glyphs[0].phys_rect[0] - (first.glyphs[0].phys_rect[0] + 40.0)).abs() < 0.01
        );
        assert!(
            (second.glyphs[0].phys_rect[1] - (first.glyphs[0].phys_rect[1] + 100.0)).abs() < 0.01
        );
        assert_eq!(second.glyphs[0].color, Color::rgb(255, 100, 50));
    }

    #[test]
    fn medium_weight_never_falls_back_to_symbol_font() {
        let mut atlas = GlyphAtlas::new_with_system_fonts();
        let resolved_family = atlas.effective_family_owned(None, false);
        let eff_w = atlas.resolve_weight_for_family(resolved_family.as_deref(), false, 500);
        let mut attrs = cosmic_text::Attrs::new().weight(cosmic_text::Weight(eff_w));
        if let Some(ref fam) = resolved_family {
            attrs = attrs.family(cosmic_text::Family::Name(fam.as_str()));
        }
        let mut buf = cosmic_text::Buffer::new(
            &mut atlas.font_system,
            cosmic_text::Metrics::new(14.0, 18.9),
        );
        {
            let mut b = buf.borrow_with(&mut atlas.font_system);
            b.set_text("Apps (6)", attrs, cosmic_text::Shaping::Advanced);
            b.shape_until_scroll(true);
        }
        let mut glyph_count = 0usize;
        for run in buf.layout_runs() {
            for g in run.glyphs {
                glyph_count += 1;
                let face = atlas
                    .font_system
                    .db()
                    .face(g.font_id)
                    .expect("shaped glyph has face");
                let is_symbol = face.families.iter().any(|(name, _)| {
                    matches!(
                        name.as_str(),
                        "Marlett" | "Webdings" | "Wingdings" | "Symbol"
                    )
                });
                assert!(
                    !is_symbol,
                    "weight(500) must not fall back to symbol font {:?}",
                    face.families
                );
            }
        }
        assert_eq!(glyph_count, "Apps (6)".len());

        let out = atlas.shape_plain_formatted_to_quads(
            "Apps (6)",
            Rect::new(0.0, 0.0, 200.0, 32.0),
            1.0,
            Color::WHITE,
            14.0,
            500,
            false,
            None,
            false,
            false,
            crate::ui::types::TextAlign::Left,
            0.0,
            1.35,
        );
        assert!(out.glyphs.len() >= 6);
    }

    #[test]
    fn glyph_atlas_overflow_repacks_without_dropping_glyphs() {
        let mut atlas = GlyphAtlas::new_fallback_only();
        let sample = "Invite Join Offline Recently Seen 12:44 AM";
        let expected_glyphs = atlas
            .shape_plain_to_quads(
                sample,
                Rect::new(0.0, 0.0, 800.0, 100.0),
                1.0,
                Color::WHITE,
                24.0,
                false,
                false,
            )
            .glyphs
            .len();
        assert!(expected_glyphs >= 30);

        // Drive atlas past 2048x2048 capacity by rasterizing many distinct large font sizes.
        let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()";
        let mut size = 36.0_f32;
        while atlas.atlas_epoch == 0 && size < 240.0 {
            let out = atlas.shape_plain_to_quads(
                alphabet,
                Rect::new(0.0, 0.0, 4000.0, 300.0),
                1.0,
                Color::WHITE,
                size,
                false,
                false,
            );
            assert!(
                out.glyphs.len() >= 60,
                "direct shaping must not drop glyphs at size {size}"
            );
            size += 2.0;
        }
        assert!(
            atlas.atlas_epoch >= 1,
            "test must exercise at least one atlas overflow reset"
        );
        assert!(matches!(
            atlas.take_dirty_upload_region(),
            Some(super::AtlasUpload::Full { .. })
        ));

        // Now verify SdfBatch::build mid-frame overflow triggers clean repack with zero dropped glyphs.
        let epoch_before = atlas.atlas_epoch;
        let mut batch = crate::gfx::sdf::SdfBatch::default();
        let mut s = 44.0_f32;
        let mut triggered_batch_reset = false;
        for _ in 0..120 {
            let mut dl = crate::ui::draw::DrawList::default();
            for row in 0..4 {
                dl.base_cmds.push(crate::ui::draw::DrawCmd::Text {
                    rect: Rect::new(10.0, 10.0 + row as f32 * 90.0, 780.0, 80.0),
                    text: if row == 0 {
                        String::from(sample)
                    } else {
                        String::from(alphabet)
                    },
                    color: Color::WHITE,
                    size: s + row as f32 * 1.5,
                    weight: 400,
                    mono: false,
                    font_family: None,
                    wrap: false,
                    ellipsis: false,
                    align: crate::ui::types::TextAlign::Left,
                    tracking: 0.0,
                    line_height: 1.35,
                    transform: [1.0, 0.0, 0.0, 0.0],
                });
            }
            batch.build(&dl, &mut atlas, 1.0, None, 800, 600);
            if atlas.atlas_epoch > epoch_before {
                triggered_batch_reset = true;
                break;
            }
            s += 2.5;
        }
        assert!(
            triggered_batch_reset,
            "batch shaping should exercise mid-frame overflow reset"
        );
        assert!(matches!(
            atlas.take_dirty_upload_region(),
            Some(super::AtlasUpload::Full { .. })
        ));

        // Every glyph of the sample string at a new size must still render completely.
        let after = atlas.shape_plain_to_quads(
            sample,
            Rect::new(0.0, 0.0, 800.0, 100.0),
            1.0,
            Color::WHITE,
            24.5,
            false,
            false,
        );
        assert_eq!(after.glyphs.len(), expected_glyphs);
        let [aw, ah] = atlas.atlas_size();
        for g in &after.glyphs {
            assert!(g.atlas_px[2] > 0 && g.atlas_px[3] > 0);
            assert!(g.atlas_px[0] + g.atlas_px[2] <= aw);
            assert!(g.atlas_px[1] + g.atlas_px[3] <= ah);
        }
    }
}
