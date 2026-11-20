use std::io::Cursor;

use crate::ui::types::Error;

const MAX_DECODE_BYTES: usize = 256 * 1024 * 1024;

/// Decoded RGBA8 pixel buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Tightly packed RGBA8 pixel buffer (`width * height * 4` bytes).
    pub data: Vec<u8>,
}

/// Encodes an RGBA8 buffer (`len == width * height * 4`) into PNG bytes.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, Error> {
    if width == 0 || height == 0 {
        return Err(Error::InvalidInput(String::from(
            "PNG dimensions must be non-zero",
        )));
    }
    let expected = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| Error::InvalidInput(String::from("PNG buffer size overflow")))?;

    if rgba.len() != expected {
        return Err(Error::InvalidInput(format!(
            "RGBA length mismatch: expected {expected}, got {}",
            rgba.len()
        )));
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::Io(format!("PNG header error: {e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| Error::Io(format!("PNG encode error: {e}")))?;
    }
    Ok(out)
}

/// Decodes PNG bytes into a normalized RGBA8 `RgbaImage`.
pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, Error> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| Error::Io(format!("PNG header decode error: {e}")))?;

    let raw_size = reader.output_buffer_size();
    if raw_size > MAX_DECODE_BYTES {
        return Err(Error::InvalidInput(String::from(
            "decoded PNG exceeds 256 MiB limit",
        )));
    }

    let mut raw = vec![0u8; raw_size];
    let info = reader
        .next_frame(&mut raw)
        .map_err(|e| Error::Io(format!("PNG frame decode error: {e}")))?;

    let pixel_count = usize::try_from(info.width)
        .ok()
        .and_then(|w| {
            usize::try_from(info.height)
                .ok()
                .and_then(|h| w.checked_mul(h))
        })
        .ok_or_else(|| Error::InvalidInput(String::from("PNG dimension overflow")))?;
    let rgba_len = pixel_count
        .checked_mul(4)
        .ok_or_else(|| Error::InvalidInput(String::from("PNG RGBA byte overflow")))?;

    let src = &raw[..info.buffer_size()];
    let mut data = Vec::with_capacity(rgba_len);

    match info.color_type {
        png::ColorType::Rgba => {
            if src.len() != rgba_len {
                return Err(Error::Io(String::from("unexpected RGBA PNG payload size")));
            }
            data.extend_from_slice(src);
        }
        png::ColorType::Rgb => {
            for chunk in src.as_chunks::<3>().0 {
                data.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for chunk in src.as_chunks::<2>().0 {
                data.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in src {
                data.extend_from_slice(&[g, g, g, 255]);
            }
        }
        other => {
            return Err(Error::Io(format!("unsupported PNG color type: {other:?}")));
        }
    }

    if data.len() != rgba_len {
        return Err(Error::Io(String::from(
            "PNG pixel conversion length mismatch",
        )));
    }

    Ok(RgbaImage {
        width: info.width,
        height: info.height,
        data,
    })
}

/// Decodes PNG or Windows `.ico` (PNG-compressed or 32-bit BGRA DIB) bytes into an RGBA8 `RgbaImage`.
pub fn decode_icon(bytes: &[u8]) -> Result<RgbaImage, Error> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return decode_png(bytes);
    }
    if bytes.len() < 6 || bytes[0..4] != [0, 0, 1, 0] {
        return decode_png(bytes);
    }

    let count = usize::from(u16::from_le_bytes([bytes[4], bytes[5]]));
    if count == 0 {
        return Err(Error::InvalidInput(String::from(
            "ICO contains zero images",
        )));
    }
    let dir_end = count
        .checked_mul(16)
        .and_then(|n| n.checked_add(6))
        .ok_or_else(|| Error::InvalidInput(String::from("ICO directory overflow")))?;
    if bytes.len() < dir_end {
        return Err(Error::InvalidInput(String::from("truncated ICO directory")));
    }

    let mut best_entry: Option<(u32, u32, usize, usize)> = None;
    for i in 0..count {
        let base = 6 + i * 16;
        let w = if bytes[base] == 0 {
            256
        } else {
            u32::from(bytes[base])
        };
        let h = if bytes[base + 1] == 0 {
            256
        } else {
            u32::from(bytes[base + 1])
        };
        let size = usize::try_from(u32::from_le_bytes([
            bytes[base + 8],
            bytes[base + 9],
            bytes[base + 10],
            bytes[base + 11],
        ]))
        .map_err(|_| Error::InvalidInput(String::from("ICO entry size overflow")))?;
        let offset = usize::try_from(u32::from_le_bytes([
            bytes[base + 12],
            bytes[base + 13],
            bytes[base + 14],
            bytes[base + 15],
        ]))
        .map_err(|_| Error::InvalidInput(String::from("ICO entry offset overflow")))?;
        let end = offset
            .checked_add(size)
            .ok_or_else(|| Error::InvalidInput(String::from("ICO entry slice overflow")))?;
        if size == 0 || end > bytes.len() {
            continue;
        }
        let area = w.saturating_mul(h);
        if best_entry.is_none_or(|(bw, bh, _, _)| area >= bw.saturating_mul(bh)) {
            best_entry = Some((w, h, offset, end));
        }
    }

    let (entry_w, entry_h, offset, end) =
        best_entry.ok_or_else(|| Error::InvalidInput(String::from("no valid ICO image entry")))?;
    let payload = &bytes[offset..end];
    if payload.starts_with(b"\x89PNG\r\n\x1a\n") {
        return decode_png(payload);
    }

    if payload.len() < 40 {
        return Err(Error::InvalidInput(String::from(
            "truncated ICO DIB header",
        )));
    }
    let header_sz = usize::try_from(u32::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))
    .unwrap_or(0);
    let bpp = u16::from_le_bytes([payload[14], payload[15]]);
    if header_sz < 40 || payload.len() < header_sz || bpp != 32 {
        return Err(Error::InvalidInput(String::from(
            "unsupported ICO DIB format (expected PNG or 32-bit BGRA)",
        )));
    }

    let row_bytes = usize::try_from(entry_w)
        .ok()
        .and_then(|w| w.checked_mul(4))
        .ok_or_else(|| Error::InvalidInput(String::from("ICO row byte overflow")))?;
    let total_bytes = usize::try_from(entry_h)
        .ok()
        .and_then(|h| h.checked_mul(row_bytes))
        .ok_or_else(|| Error::InvalidInput(String::from("ICO pixel byte overflow")))?;
    let xor_end = header_sz
        .checked_add(total_bytes)
        .ok_or_else(|| Error::InvalidInput(String::from("ICO XOR mask overflow")))?;
    if payload.len() < xor_end || total_bytes > MAX_DECODE_BYTES {
        return Err(Error::InvalidInput(String::from(
            "truncated ICO pixel data",
        )));
    }

    let xor_pixels = &payload[header_sz..xor_end];
    let mut data = vec![0u8; total_bytes];
    let h_usize = entry_h as usize;
    for row in 0..h_usize {
        let src_row = &xor_pixels[(h_usize - 1 - row) * row_bytes..(h_usize - row) * row_bytes];
        let dst_row = &mut data[row * row_bytes..(row + 1) * row_bytes];
        for (src_px, dst_px) in src_row
            .as_chunks::<4>()
            .0
            .iter()
            .zip(dst_row.as_chunks_mut::<4>().0)
        {
            dst_px[0] = src_px[2];
            dst_px[1] = src_px[1];
            dst_px[2] = src_px[0];
            dst_px[3] = src_px[3];
        }
    }

    Ok(RgbaImage {
        width: entry_w,
        height: entry_h,
        data,
    })
}

fn decode_bmp(bytes: &[u8]) -> Result<RgbaImage, Error> {
    if bytes.len() < 54 || &bytes[0..2] != b"BM" {
        return Err(Error::InvalidInput(String::from("invalid BMP header")));
    }
    let pixel_offset = usize::try_from(u32::from_le_bytes([
        bytes[10], bytes[11], bytes[12], bytes[13],
    ]))
    .map_err(|_| Error::InvalidInput(String::from("BMP offset overflow")))?;
    let w_i32 = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let h_i32 = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    let bpp = u16::from_le_bytes([bytes[28], bytes[29]]);
    if w_i32 <= 0 || h_i32 == 0 || (bpp != 24 && bpp != 32) {
        return Err(Error::InvalidInput(String::from(
            "unsupported BMP dimensions or bit depth",
        )));
    }
    let top_down = h_i32 < 0;
    let width = w_i32 as u32;
    let height = h_i32.unsigned_abs();
    let bytes_per_px = (bpp / 8) as usize;
    let raw_row = (width as usize)
        .checked_mul(bytes_per_px)
        .ok_or_else(|| Error::InvalidInput(String::from("BMP row overflow")))?;
    let padded_row = (raw_row + 3) & !3;
    let total_src = padded_row
        .checked_mul(height as usize)
        .and_then(|n| n.checked_add(pixel_offset))
        .ok_or_else(|| Error::InvalidInput(String::from("BMP data size overflow")))?;
    if bytes.len() < total_src {
        return Err(Error::InvalidInput(String::from(
            "truncated BMP pixel data",
        )));
    }
    let out_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| Error::InvalidInput(String::from("BMP output overflow")))?;
    if out_len > MAX_DECODE_BYTES {
        return Err(Error::InvalidInput(String::from(
            "BMP exceeds 256 MiB limit",
        )));
    }

    let mut data = vec![0u8; out_len];
    let h_usize = height as usize;
    let w_usize = width as usize;
    for y in 0..h_usize {
        let src_y = if top_down { y } else { h_usize - 1 - y };
        let src_row =
            &bytes[pixel_offset + src_y * padded_row..pixel_offset + src_y * padded_row + raw_row];
        let dst_row = &mut data[y * w_usize * 4..(y + 1) * w_usize * 4];
        for x in 0..w_usize {
            let s = x * bytes_per_px;
            let d = x * 4;
            dst_row[d] = src_row[s + 2];
            dst_row[d + 1] = src_row[s + 1];
            dst_row[d + 2] = src_row[s];
            dst_row[d + 3] = if bytes_per_px == 4 {
                src_row[s + 3]
            } else {
                255
            };
        }
    }
    Ok(RgbaImage {
        width,
        height,
        data,
    })
}

/// Decoded frame from a multi-frame animated GIF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifFrame {
    /// Decoded RGBA8 pixel buffer for this frame.
    pub image: RgbaImage,
    /// Recommended frame display delay in milliseconds.
    pub delay_ms: u32,
}

fn decode_gif_inner(bytes: &[u8], first_only: bool) -> Result<Vec<GifFrame>, Error> {
    if bytes.len() < 13 || (&bytes[0..6] != b"GIF87a" && &bytes[0..6] != b"GIF89a") {
        return Err(Error::InvalidInput(String::from("invalid GIF header")));
    }
    let screen_w = u32::from(u16::from_le_bytes([bytes[6], bytes[7]])).max(1);
    let screen_h = u32::from(u16::from_le_bytes([bytes[8], bytes[9]])).max(1);
    let packed = bytes[10];
    let mut pos = 13usize;
    let mut global_pal = [[0u8; 3]; 256];
    if (packed & 0x80) != 0 {
        let gct_count = 1usize << ((packed & 0x07) + 1);
        let gct_bytes = gct_count * 3;
        if bytes.len() < pos + gct_bytes {
            return Err(Error::InvalidInput(String::from(
                "truncated GIF global color table",
            )));
        }
        for i in 0..gct_count {
            global_pal[i] = [
                bytes[pos + i * 3],
                bytes[pos + i * 3 + 1],
                bytes[pos + i * 3 + 2],
            ];
        }
        pos += gct_bytes;
    }

    let canvas_pixels = usize::try_from(screen_w)
        .ok()
        .and_then(|w| {
            usize::try_from(screen_h)
                .ok()
                .and_then(|h| w.checked_mul(h))
        })
        .ok_or_else(|| Error::InvalidInput(String::from("GIF canvas dimension overflow")))?;
    let canvas_bytes = canvas_pixels
        .checked_mul(4)
        .ok_or_else(|| Error::InvalidInput(String::from("GIF canvas RGBA overflow")))?;
    if canvas_bytes > MAX_DECODE_BYTES {
        return Err(Error::InvalidInput(String::from(
            "GIF canvas exceeds size cap",
        )));
    }

    let mut canvas = vec![0u8; canvas_bytes];
    let mut prev_canvas = Vec::new();
    let mut frames = Vec::new();
    let mut transparent_idx: Option<u8> = None;
    let mut delay_ms: u32 = 100;
    let mut disposal: u8 = 0;

    while pos < bytes.len() {
        let marker = bytes[pos];
        pos += 1;
        match marker {
            0x21 => {
                // Extension block
                let ext_label = *bytes
                    .get(pos)
                    .ok_or_else(|| Error::InvalidInput(String::from("truncated GIF extension")))?;
                pos += 1;
                if ext_label == 0xF9 && bytes.len() >= pos + 5 {
                    let gce_packed = bytes[pos + 1];
                    disposal = (gce_packed >> 2) & 0x07;
                    let cs = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]);
                    delay_ms = u32::from(cs).saturating_mul(10);
                    transparent_idx = if (gce_packed & 0x01) != 0 {
                        Some(bytes[pos + 4])
                    } else {
                        None
                    };
                }
                while let Some(&sub_len) = bytes.get(pos) {
                    pos += 1;
                    if sub_len == 0 {
                        break;
                    }
                    pos = pos.saturating_add(usize::from(sub_len));
                }
            }
            0x2C => {
                // Image descriptor
                if bytes.len() < pos + 9 {
                    return Err(Error::InvalidInput(String::from(
                        "truncated GIF image descriptor",
                    )));
                }
                let img_x = u32::from(u16::from_le_bytes([bytes[pos], bytes[pos + 1]]));
                let img_y = u32::from(u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]));
                let img_w = u32::from(u16::from_le_bytes([bytes[pos + 4], bytes[pos + 5]]));
                let img_h = u32::from(u16::from_le_bytes([bytes[pos + 6], bytes[pos + 7]]));
                let img_packed = bytes[pos + 8];
                pos += 9;
                let mut pal = global_pal;
                if (img_packed & 0x80) != 0 {
                    let lct_count = 1usize << ((img_packed & 0x07) + 1);
                    let lct_bytes = lct_count * 3;
                    if bytes.len() < pos + lct_bytes {
                        return Err(Error::InvalidInput(String::from(
                            "truncated GIF local palette",
                        )));
                    }
                    for i in 0..lct_count {
                        pal[i] = [
                            bytes[pos + i * 3],
                            bytes[pos + i * 3 + 1],
                            bytes[pos + i * 3 + 2],
                        ];
                    }
                    pos += lct_bytes;
                }
                let min_code_size = *bytes.get(pos).ok_or_else(|| {
                    Error::InvalidInput(String::from("missing GIF LZW code size"))
                })?;
                pos += 1;
                if !(2..=8).contains(&min_code_size) {
                    return Err(Error::InvalidInput(String::from(
                        "invalid GIF LZW code size",
                    )));
                }
                let mut lzw_stream = Vec::new();
                while let Some(&sub_len) = bytes.get(pos) {
                    pos += 1;
                    if sub_len == 0 {
                        break;
                    }
                    let end = pos.saturating_add(usize::from(sub_len));
                    if end > bytes.len() {
                        return Err(Error::InvalidInput(String::from("truncated GIF LZW block")));
                    }
                    lzw_stream.extend_from_slice(&bytes[pos..end]);
                    pos = end;
                }

                let pixel_count = usize::try_from(img_w)
                    .ok()
                    .and_then(|w| usize::try_from(img_h).ok().and_then(|h| w.checked_mul(h)))
                    .ok_or_else(|| Error::InvalidInput(String::from("GIF dimension overflow")))?;
                if pixel_count == 0 || pixel_count.saturating_mul(4) > MAX_DECODE_BYTES {
                    return Err(Error::InvalidInput(String::from("invalid GIF image size")));
                }

                let clear_code = 1u16 << min_code_size;
                let end_code = clear_code + 1;
                let mut code_size = u32::from(min_code_size) + 1;
                let mut next_code = end_code + 1;
                let mut dict_prefix = [0u16; 4096];
                let mut dict_suffix = [0u8; 4096];
                let mut dict_len = [0u16; 4096];
                for i in 0..clear_code {
                    dict_suffix[i as usize] = i as u8;
                    dict_len[i as usize] = 1;
                }

                let mut bit_pos = 0usize;
                let total_bits = lzw_stream.len() * 8;
                let mut prev_code: Option<u16> = None;
                let mut indices = Vec::with_capacity(pixel_count);
                let mut stack = [0u8; 4096];

                while bit_pos + (code_size as usize) <= total_bits && indices.len() < pixel_count {
                    let mut code = 0u32;
                    for b in 0..(code_size as usize) {
                        let bit_idx = bit_pos + b;
                        let byte_val = lzw_stream[bit_idx >> 3];
                        let bit_val = (byte_val >> (bit_idx & 7)) & 1;
                        code |= u32::from(bit_val) << b;
                    }
                    bit_pos += code_size as usize;
                    let code = code as u16;

                    if code == clear_code {
                        code_size = u32::from(min_code_size) + 1;
                        next_code = end_code + 1;
                        prev_code = None;
                        continue;
                    }
                    if code == end_code {
                        break;
                    }

                    let mut cur = if code < next_code {
                        code
                    } else if code == next_code {
                        let Some(p) = prev_code else {
                            break;
                        };
                        p
                    } else {
                        break;
                    };

                    let mut sp = 0usize;
                    while cur >= clear_code && sp < 4095 {
                        stack[sp] = dict_suffix[cur as usize];
                        sp += 1;
                        cur = dict_prefix[cur as usize];
                    }
                    let first_byte = dict_suffix[cur as usize];
                    stack[sp] = first_byte;
                    sp += 1;

                    for &b in stack[..sp].iter().rev() {
                        if indices.len() < pixel_count {
                            indices.push(b);
                        }
                    }
                    if code == next_code && indices.len() < pixel_count {
                        indices.push(first_byte);
                    }

                    if let Some(p) = prev_code {
                        if next_code < 4096 {
                            dict_prefix[next_code as usize] = p;
                            dict_suffix[next_code as usize] = first_byte;
                            dict_len[next_code as usize] = dict_len[p as usize].saturating_add(1);
                            next_code += 1;
                            if next_code == (1u16 << code_size) && code_size < 12 {
                                code_size += 1;
                            }
                        }
                    }
                    prev_code = Some(code);
                }

                if disposal == 3 {
                    prev_canvas = canvas.clone();
                }

                let interlaced = (img_packed & 0x40) != 0;
                let pass_starts = [0u32, 4, 2, 1];
                let pass_steps = [8u32, 8, 4, 2];
                let mut src_row = 0u32;
                let mut row_map = Vec::new();
                if interlaced {
                    row_map.resize(img_h as usize, 0);
                    for p in 0..4 {
                        let mut y = pass_starts[p];
                        while y < img_h {
                            if (src_row as usize) < row_map.len() {
                                row_map[src_row as usize] = y;
                            }
                            src_row += 1;
                            y += pass_steps[p];
                        }
                    }
                }

                for sy in 0..img_h {
                    let dst_y = img_y
                        + if interlaced {
                            row_map.get(sy as usize).copied().unwrap_or(sy)
                        } else {
                            sy
                        };
                    if dst_y >= screen_h {
                        continue;
                    }
                    for sx in 0..img_w {
                        let dst_x = img_x + sx;
                        if dst_x >= screen_w {
                            continue;
                        }
                        let idx_pos = (sy as usize) * (img_w as usize) + (sx as usize);
                        let Some(&idx) = indices.get(idx_pos) else {
                            continue;
                        };
                        if Some(idx) == transparent_idx {
                            continue;
                        }
                        let [r, g, b] = pal[idx as usize];
                        let dst = ((dst_y as usize) * (screen_w as usize) + (dst_x as usize)) * 4;
                        canvas[dst..dst + 4].copy_from_slice(&[r, g, b, 255]);
                    }
                }

                frames.push(GifFrame {
                    image: RgbaImage {
                        width: screen_w,
                        height: screen_h,
                        data: canvas.clone(),
                    },
                    delay_ms,
                });
                if first_only {
                    return Ok(frames);
                }

                match disposal {
                    2 => {
                        for sy in 0..img_h {
                            let dst_y = img_y + sy;
                            if dst_y >= screen_h {
                                continue;
                            }
                            for sx in 0..img_w {
                                let dst_x = img_x + sx;
                                if dst_x >= screen_w {
                                    continue;
                                }
                                let dst =
                                    ((dst_y as usize) * (screen_w as usize) + (dst_x as usize)) * 4;
                                canvas[dst..dst + 4].copy_from_slice(&[0, 0, 0, 0]);
                            }
                        }
                    }
                    3 if !prev_canvas.is_empty() => {
                        canvas.copy_from_slice(&prev_canvas);
                    }
                    _ => {}
                }
                transparent_idx = None;
                delay_ms = 100;
                disposal = 0;
            }
            0x3B => break,
            _ => break,
        }
    }
    if frames.is_empty() {
        Err(Error::InvalidInput(String::from("no valid frame in GIF")))
    } else {
        Ok(frames)
    }
}

/// Decodes all frames of an animated (or single-frame) GIF87a/89a image.
pub fn decode_gif_frames(bytes: &[u8]) -> Result<Vec<GifFrame>, Error> {
    decode_gif_inner(bytes, false)
}

fn decode_gif(bytes: &[u8]) -> Result<RgbaImage, Error> {
    decode_gif_inner(bytes, true)?
        .into_iter()
        .next()
        .map(|f| f.image)
        .ok_or_else(|| Error::InvalidInput(String::from("no valid frame in GIF")))
}

fn decode_jpeg(bytes: &[u8]) -> Result<RgbaImage, Error> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    use zune_jpeg::JpegDecoder;

    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut dec = JpegDecoder::new_with_options(bytes, opts);
    dec.decode_headers()
        .map_err(|e| Error::InvalidInput(format!("invalid JPEG header: {e}")))?;
    let info = dec
        .info()
        .ok_or_else(|| Error::InvalidInput(String::from("missing JPEG dimensions")))?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    let byte_len = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| Error::InvalidInput(String::from("JPEG dimension overflow")))?;
    if byte_len == 0 || byte_len > MAX_DECODE_BYTES {
        return Err(Error::InvalidInput(String::from(
            "JPEG exceeds decode size cap",
        )));
    }
    let data = dec
        .decode()
        .map_err(|e| Error::InvalidInput(format!("JPEG decode failed: {e}")))?;
    if data.len() != byte_len {
        return Err(Error::InvalidInput(String::from(
            "JPEG RGBA output size mismatch",
        )));
    }
    Ok(RgbaImage {
        width,
        height,
        data,
    })
}

fn decode_webp(bytes: &[u8]) -> Result<RgbaImage, Error> {
    use image_webp::WebPDecoder;

    let mut dec = WebPDecoder::new(Cursor::new(bytes))
        .map_err(|e| Error::InvalidInput(format!("invalid WebP: {e}")))?;
    let (width, height) = dec.dimensions();
    let has_alpha = dec.has_alpha();
    let bpp = if has_alpha { 4usize } else { 3usize };
    let px_count = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .ok_or_else(|| Error::InvalidInput(String::from("WebP dimension overflow")))?;
    let rgba_len = px_count
        .checked_mul(4)
        .ok_or_else(|| Error::InvalidInput(String::from("WebP RGBA overflow")))?;
    if rgba_len == 0 || rgba_len > MAX_DECODE_BYTES {
        return Err(Error::InvalidInput(String::from(
            "WebP exceeds decode size cap",
        )));
    }
    let mut raw = vec![0u8; px_count.saturating_mul(bpp)];
    dec.read_image(&mut raw)
        .map_err(|e| Error::InvalidInput(format!("WebP decode failed: {e}")))?;
    let data = if has_alpha {
        raw
    } else {
        let mut rgba = Vec::with_capacity(rgba_len);
        for rgb in raw.as_chunks::<3>().0 {
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        rgba
    };
    Ok(RgbaImage {
        width,
        height,
        data,
    })
}

/// Decodes PNG, JPEG, WebP, Windows `.ico`, BMP (24/32-bit), or GIF (87a/89a) bytes into an RGBA8 `RgbaImage`.
pub fn decode_image(bytes: &[u8]) -> Result<RgbaImage, Error> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return decode_png(bytes);
    }
    if bytes.len() >= 3 && bytes[0..3] == [0xFF, 0xD8, 0xFF] {
        return decode_jpeg(bytes);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return decode_webp(bytes);
    }
    if bytes.len() >= 6 && bytes[0..4] == [0, 0, 1, 0] {
        return decode_icon(bytes);
    }
    if bytes.starts_with(b"BM") {
        return decode_bmp(bytes);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return decode_gif(bytes);
    }
    decode_png(bytes)
}

#[cfg(test)]
mod tests {
    use super::{decode_gif_frames, decode_icon, decode_image, decode_png, encode_png};

    #[test]
    fn png_encode_decode_roundtrip_and_bounds() {
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, // row 0
            0, 0, 255, 128, 255, 255, 255, 255, // row 1
        ];
        let png_bytes = encode_png(2, 2, &rgba).expect("encode succeeds");
        let decoded = decode_png(&png_bytes).expect("decode succeeds");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.data, rgba);

        // ICO wrapping a 32-bit BGRA DIB (1x1 pixel: R=10, G=20, B=30, A=255 -> BGRA = [30, 20, 10, 255])
        let mut ico = Vec::new();
        ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]); // ICONDIR: 1 icon
        ico.extend_from_slice(&[1, 1, 0, 0, 1, 0, 32, 0]); // 1x1, 1 plane, 32 bpp
        ico.extend_from_slice(&48u32.to_le_bytes()); // 40-byte DIB header + 4-byte XOR + 4-byte AND
        ico.extend_from_slice(&22u32.to_le_bytes()); // offset = 6 + 16 = 22
        ico.extend_from_slice(&40u32.to_le_bytes()); // biSize = 40
        ico.extend_from_slice(&1i32.to_le_bytes()); // biWidth = 1
        ico.extend_from_slice(&2i32.to_le_bytes()); // biHeight = 2 (XOR + AND)
        ico.extend_from_slice(&1u16.to_le_bytes()); // biPlanes = 1
        ico.extend_from_slice(&32u16.to_le_bytes()); // biBitCount = 32
        ico.extend_from_slice(&[0u8; 24]); // remaining BITMAPINFOHEADER fields
        ico.extend_from_slice(&[30, 20, 10, 255]); // 1x1 BGRA pixel
        ico.extend_from_slice(&[0, 0, 0, 0]); // AND mask padding
        let ico_img = decode_icon(&ico).expect("decode dib ico");
        assert_eq!(ico_img.width, 1);
        assert_eq!(ico_img.height, 1);
        assert_eq!(ico_img.data, vec![10, 20, 30, 255]);

        // 1x1 32-bit BMP
        let mut bmp = vec![0u8; 58];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&58u32.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&1i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&1i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
        bmp[54..58].copy_from_slice(&[90, 80, 70, 255]);
        let bmp_img = decode_image(&bmp).expect("decode bmp");
        assert_eq!(bmp_img.data, vec![70, 80, 90, 255]);

        // 1x1 GIF89a (palette entry 0 = [12, 34, 56], LZW min_code_size=2, stream=[clear(4), 0, end(5)] = 0x44, 0x01)
        let gif = [
            b'G', b'I', b'F', b'8', b'9', b'a', 1, 0, 1, 0, 0x80, 0,
            0, // header + 2-color GCT
            12, 34, 56, 0, 0, 0, // GCT: color 0 = (12,34,56), color 1 = (0,0,0)
            0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, // Image Descriptor 1x1
            2, 2, 0x44, 0x01,
            0, // LZW min_code_size=2, 2 bytes: [clear=4 (100), idx=0 (000), eoi=5 (101)]
            0x3B,
        ];
        let gif_img = decode_image(&gif).expect("decode gif");
        assert_eq!(gif_img.width, 1);
        assert_eq!(gif_img.height, 1);
        assert_eq!(gif_img.data, vec![12, 34, 56, 255]);

        assert!(encode_png(0, 10, &[]).is_err());
        assert!(encode_png(u32::MAX, u32::MAX, &[]).is_err());

        // 2-frame animated 1x1 GIF89a with GCE delays (50ms, 120ms) and palette index 0 then 1.
        let anim_gif = [
            b'G', b'I', b'F', b'8', b'9', b'a', 1, 0, 1, 0, 0x80, 0, 0, 10, 20, 30, 200, 100,
            50, // GCT: 0=(10,20,30), 1=(200,100,50)
            0x21, 0xF9, 4, 0, 5, 0, 0, 0, // GCE: delay=5 (50ms)
            0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 0x01, 0, // idx 0
            0x21, 0xF9, 4, 0, 12, 0, 0, 0, // GCE: delay=12 (120ms)
            0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x4C, 0x01,
            0, // [clear=4(100), idx=1(001), eoi=5(101)] -> 0b01001100=0x4C, 0b00000001=0x01
            0x3B,
        ];
        let frames = decode_gif_frames(&anim_gif).expect("decode animated gif");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].delay_ms, 50);
        assert_eq!(frames[0].image.data, vec![10, 20, 30, 255]);
        assert_eq!(frames[1].delay_ms, 120);
        assert_eq!(frames[1].image.data, vec![200, 100, 50, 255]);
    }
}
