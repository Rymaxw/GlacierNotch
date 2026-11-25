use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use eframe::egui;
use egui::Color32;

struct Thumbnail {
    rgba: image::RgbaImage,
    accent: Option<Color32>,
}

struct Loaded {
    key: String,
    thumbnail: Option<Thumbnail>,
}

pub struct Artwork {
    tx: Sender<(String, PathBuf)>,
    rx: Receiver<Loaded>,
    key: String,
    texture: Option<egui::TextureHandle>,
    accent: Option<Color32>,
}

impl Artwork {
    pub fn new() -> Self {
        let (tx, requests) = channel::<(String, PathBuf)>();
        let (answers, rx) = channel::<Loaded>();
        thread::Builder::new()
            .name("glacier-artwork".into())
            .spawn(move || {
                while let Ok((key, path)) = requests.recv() {
                    let thumbnail = load_thumbnail(&path);
                    if answers.send(Loaded { key, thumbnail }).is_err() {
                        break;
                    }
                }
            })
            .expect("failed to start artwork worker");
        Self {
            tx,
            rx,
            key: String::new(),
            texture: None,
            accent: None,
        }
    }

    pub fn update(&mut self, ctx: &egui::Context, url: Option<&str>) {
        let key = url.unwrap_or_default();
        if key != self.key {
            self.key = key.to_owned();
            self.texture = None;
            self.accent = None;
            if let Some(path) = url.and_then(crate::media::local_art_path) {
                let _ = self.tx.send((self.key.clone(), path));
            }
        }

        while let Ok(loaded) = self.rx.try_recv() {
            if loaded.key != self.key {
                continue;
            }
            if let Some(Thumbnail { rgba, accent }) = loaded.thumbnail {
                let size = [rgba.width() as usize, rgba.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.accent = accent;
                self.texture = Some(ctx.load_texture(
                    "glacier-album-art",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
    }

    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        self.texture.as_ref()
    }

    pub fn accent(&self) -> Option<Color32> {
        self.accent
    }
}

fn load_thumbnail(path: &std::path::Path) -> Option<Thumbnail> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
        return None;
    }
    let mut reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.thumbnail(160, 160).to_rgba8();
    let accent = cover_accent(&rgba);
    Some(Thumbnail { rgba, accent })
}

fn cover_accent(image: &image::RgbaImage) -> Option<Color32> {
    const BINS: usize = 18;
    let mut buckets = [[0_u32; 4]; BINS];
    for &image::Rgba([r, g, b, a]) in image.pixels() {
        let high = r.max(g).max(b);
        let chroma = high - r.min(g).min(b);
        if a < 128 || high < 40 || chroma < 22 {
            continue;
        }
        let hue = egui::ecolor::Hsva::from_srgb([r, g, b]).h;
        let bucket = &mut buckets[(hue * BINS as f32) as usize % BINS];
        let weight = u32::from(a) * u32::from(chroma) / 255;
        for (sum, channel) in bucket[..3].iter_mut().zip([r, g, b]) {
            *sum += u32::from(channel) * weight;
        }
        bucket[3] += weight;
    }
    let [r, g, b, weight] = buckets.into_iter().max_by_key(|bucket| bucket[3])?;
    if weight == 0 {
        return None;
    }
    let hsv =
        egui::ecolor::Hsva::from_srgb([(r / weight) as u8, (g / weight) as u8, (b / weight) as u8]);
    Some(readable_accent(hsv.h, hsv.s))
}

pub fn readable_accent(hue: f32, saturation: f32) -> Color32 {
    let [r, g, b] = egui::ecolor::Hsva::new(hue, saturation.clamp(0.55, 0.92), 0.94, 1.0).to_rgb();
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    // 0.23 leaves at least 5:1 contrast on the notch's near-black surface.
    let white = ((0.23 - luminance) / (1.0 - luminance)).max(0.0);
    egui::Rgba::from_rgb(
        r + (1.0 - r) * white,
        g + (1.0 - g) * white,
        b + (1.0 - b) * white,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_prefers_dominant_visible_color_over_neutral_and_transparent_pixels() {
        let mut cover = image::RgbaImage::from_pixel(20, 20, image::Rgba([245, 245, 245, 255]));
        for (index, pixel) in cover.pixels_mut().enumerate() {
            *pixel = match index {
                0..70 => image::Rgba([18, 53, 165, 255]),
                70..85 => image::Rgba([255, 98, 10, 255]),
                85..260 => image::Rgba([250, 25, 17, 0]),
                _ => *pixel,
            };
        }
        let color = cover_accent(&cover).unwrap();
        assert!(color.b() > color.g() && color.g() > color.r());
        let [r, g, b, _] = egui::Rgba::from(color).to_array();
        assert!(0.2126 * r + 0.7152 * g + 0.0722 * b >= 0.225);
    }

    #[test]
    fn monochrome_cover_has_no_hue() {
        let cover = image::RgbaImage::from_pixel(8, 8, image::Rgba([180, 180, 180, 255]));
        assert_eq!(cover_accent(&cover), None);
    }
}
