//! Image and text preprocessing matching the Hugging Face SigLIP 2 processor.
//!
//! Images: decode, apply EXIF orientation, drop alpha, bilinear resize to a
//! square, rescale to [0, 1], normalize with the manifest mean/std, CHW f32.
//! Text: lowercase (the model was trained on lowercased text), tokenize with
//! the Gemma tokenizer, append EOS, truncate and pad to a fixed length.

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader, Limits, imageops::FilterType};
use thiserror::Error;
use tokenizers::{Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy};

use crate::manifest::Manifest;

/// Decoded images may not allocate more than this, guarding against
/// decompression bombs in small request bodies.
const MAX_DECODE_ALLOC: u64 = 512 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;

#[derive(Debug, Error)]
pub enum PreprocessError {
    #[error("image could not be decoded: {0}")]
    UndecodableImage(String),
    #[error("tokenizer failed: {0}")]
    Tokenizer(String),
}

#[derive(Debug, Clone)]
pub struct ImageSpec {
    pub size: u32,
    pub mean: [f32; 3],
    pub std: [f32; 3],
    pub rescale: f32,
}

impl From<&Manifest> for ImageSpec {
    fn from(manifest: &Manifest) -> Self {
        Self {
            size: manifest.image_size,
            mean: manifest.image_mean,
            std: manifest.image_std,
            rescale: manifest.rescale_factor,
        }
    }
}

/// Decode an uploaded image, applying its EXIF orientation.
pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, PreprocessError> {
    let undecodable =
        |error: image::ImageError| PreprocessError::UndecodableImage(error.to_string());
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| PreprocessError::UndecodableImage(error.to_string()))?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(undecodable)?;
    let orientation = decoder.orientation().map_err(undecodable)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(undecodable)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// Convert an image to the model's normalized CHW f32 input.
#[must_use]
pub fn image_tensor(image: &DynamicImage, spec: &ImageSpec) -> Vec<f32> {
    let rgb = image.to_rgb8();
    let resized = image::imageops::resize(&rgb, spec.size, spec.size, FilterType::Triangle);
    let plane = (spec.size * spec.size) as usize;
    let mut tensor = vec![0.0_f32; plane * 3];
    for (index, pixel) in resized.pixels().enumerate() {
        for channel in 0..3 {
            let value = f32::from(pixel[channel]) * spec.rescale;
            tensor[channel * plane + index] = (value - spec.mean[channel]) / spec.std[channel];
        }
    }
    tensor
}

/// Gemma tokenizer configured for SigLIP 2 text input.
pub struct TextEncoder {
    tokenizer: Tokenizer,
    max_tokens: usize,
    lowercase: bool,
}

impl TextEncoder {
    pub fn from_file(path: &Path, manifest: &Manifest) -> anyhow::Result<Self> {
        let mut tokenizer = Tokenizer::from_file(path)
            .map_err(|error| anyhow::anyhow!("loading {}: {error}", path.display()))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: manifest.text_max_tokens,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                direction: TruncationDirection::Right,
            }))
            .map_err(|error| anyhow::anyhow!("configuring truncation: {error}"))?;
        Ok(Self {
            tokenizer,
            max_tokens: manifest.text_max_tokens,
            lowercase: manifest.text_lowercase,
        })
    }

    /// Token IDs padded or truncated to exactly `max_tokens`, ending in EOS
    /// before any padding.
    pub fn encode(&self, text: &str) -> Result<Vec<i64>, PreprocessError> {
        let text = if self.lowercase {
            text.to_lowercase()
        } else {
            text.to_owned()
        };
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|error| PreprocessError::Tokenizer(error.to_string()))?;
        let mut ids: Vec<i64> = encoding.get_ids().iter().map(|id| i64::from(*id)).collect();
        // The tokenizer file pads to a fixed length itself; enforce it anyway so
        // a different tokenizer file cannot change the model input shape.
        ids.resize(self.max_tokens, 0);
        Ok(ids)
    }

    #[must_use]
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn spec(size: u32) -> ImageSpec {
        ImageSpec {
            size,
            mean: [0.5; 3],
            std: [0.5; 3],
            rescale: 1.0 / 255.0,
        }
    }

    #[test]
    fn tensor_is_chw_and_normalized_to_unit_range() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([255, 0, 128])));
        let tensor = image_tensor(&image, &spec(4));
        assert_eq!(tensor.len(), 3 * 16);
        assert!(tensor[..16].iter().all(|value| (*value - 1.0).abs() < 1e-6));
        assert!(
            tensor[16..32]
                .iter()
                .all(|value| (*value + 1.0).abs() < 1e-6)
        );
        assert!(tensor[32..].iter().all(|value| value.abs() < 0.01));
    }

    #[test]
    fn alpha_is_dropped_not_composited() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 0])));
        let tensor = image_tensor(&image, &spec(4));
        assert!(tensor.iter().all(|value| (*value - 1.0).abs() < 1e-6));
    }

    #[test]
    fn garbage_bytes_are_rejected() {
        assert!(matches!(
            decode_image(b"definitely not an image"),
            Err(PreprocessError::UndecodableImage(_))
        ));
        assert!(matches!(
            decode_image(&[]),
            Err(PreprocessError::UndecodableImage(_))
        ));
    }

    #[test]
    fn png_round_trip_decodes() {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::from_pixel(3, 2, Rgb([1, 2, 3])))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let image = decode_image(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (3, 2));
    }
}
