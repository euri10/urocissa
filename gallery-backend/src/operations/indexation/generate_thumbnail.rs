use crate::{
    operations::{
        indexation::generate_dynamic_image::decode_image,
        indexation::generate_ffmpeg::create_silent_ffmpeg_command,
        utils::resize::{max_long_side_width_height, small_width_height},
    },
    public::structure::abstract_data::AbstractData,
};
use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, ImageFormat};
use std::{path::Path, process::Stdio};

/// Long side of the viewer-sized artifact under `object/compressed`.
pub const VIEWER_LONG_SIDE: u32 = 1920;

/// Long side of the grid-sized artifact under `object/thumbnail`.
///
/// The gallery lays rows out at `subRowHeightScale` CSS pixels, which the
/// frontend clamps to 250..=450. 768 covers the default 250 px row at a device
/// pixel ratio of 2 for ordinary aspect ratios, and every row height at a ratio
/// of 1. Tiles needing more than this fall back to the viewer artifact rather
/// than being upscaled.
pub const GRID_LONG_SIDE: u32 = 768;

fn write_jpeg_thumbnail(
    dynamic_image: &DynamicImage,
    width: u32,
    height: u32,
    output_path: &Path,
) -> Result<()> {
    let thumbnail_image = dynamic_image.thumbnail_exact(width, height).to_rgb8();
    #[cfg(test)]
    crate::process::staging_failure_tests::jpeg_write_failure(&thumbnail_image, output_path)?;

    // Resolve parent directory of the output path
    let parent_path = output_path.parent().ok_or_else(|| {
        anyhow!(
            "failed to determine parent directory of {}",
            output_path.display()
        )
    })?;

    // Ensure the directory exists
    std::fs::create_dir_all(parent_path).context(format!(
        "failed to create directory tree {}",
        parent_path.display()
    ))?;

    // Persist the thumbnail as JPEG
    thumbnail_image
        .save_with_format(output_path, ImageFormat::Jpeg)
        .context(format!(
            "failed to save JPEG thumbnail to {}",
            output_path.display()
        ))?;

    Ok(())
}

pub fn generate_thumbnail_for_image_to(
    abstract_data: &AbstractData,
    dynamic_image: &DynamicImage,
    output_path: &Path,
) -> Result<()> {
    let (compressed_width, compressed_height) = max_long_side_width_height(
        abstract_data.width(),
        abstract_data.height(),
        VIEWER_LONG_SIDE,
    );

    write_jpeg_thumbnail(
        dynamic_image,
        compressed_width,
        compressed_height,
        output_path,
    )
}

/// Derive the grid-sized artifact from an already written viewer-sized one.
///
/// Both image and video pipelines share this, so the two tiers can never
/// disagree about orientation or frame choice, and the source is decoded once
/// from a JPEG rather than from the original media.
///
/// # Errors
///
/// Returns an error when `source_path` cannot be decoded, or when the resized
/// image cannot be written to `output_path`.
pub fn generate_small_thumbnail_from(source_path: &Path, output_path: &Path) -> Result<()> {
    let dynamic_image = decode_image(source_path).context(format!(
        "failed to decode thumbnail {} for the grid tier",
        source_path.display()
    ))?;
    let (width, height) = max_long_side_width_height(
        dynamic_image.width(),
        dynamic_image.height(),
        GRID_LONG_SIDE,
    );

    write_jpeg_thumbnail(&dynamic_image, width, height, output_path)
}

pub fn generate_thumbnail_for_video_to(
    abstract_data: &AbstractData,
    output_path: &Path,
) -> Result<()> {
    #[cfg(test)]
    crate::process::staging_failure_tests::partial_write_failure(output_path)?;

    let (width, height) = (abstract_data.width(), abstract_data.height());
    let (thumb_width, thumb_height) = small_width_height(width, height, 1280);

    // Create target directory tree if missing
    std::fs::create_dir_all(abstract_data.compressed_path_parent())
        .context("failed to create parent directory for video thumbnail")?;

    // Assemble silent ffmpeg command
    let mut cmd = create_silent_ffmpeg_command();
    cmd.args([
        "-y",
        "-i",
        &abstract_data.imported_path_string(),
        "-ss",
        "0",
        "-vframes",
        "1",
        "-vf",
        &format!("scale={thumb_width}:{thumb_height}"),
        &output_path.to_string_lossy(),
    ]);

    // Execute and wait; we discard both stdout/stderr
    let status = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to execute ffmpeg for video thumbnail generation")?;

    if !status.success() {
        return Err(anyhow!(
            "ffmpeg thumbnail generation failed with exit code: {}",
            status.code().unwrap_or(-1)
        ));
    }

    Ok(())
}

#[cfg(test)]
mod small_thumbnail_tests {
    use super::{GRID_LONG_SIDE, generate_small_thumbnail_from};
    use crate::process::media_pipeline as backfill;
    use image::{DynamicImage, ImageFormat, RgbImage};

    fn write_jpeg(directory: &std::path::Path, width: u32, height: u32) -> std::path::PathBuf {
        let path = directory.join("viewer.jpg");
        DynamicImage::ImageRgb8(RgbImage::new(width, height))
            .save_with_format(&path, ImageFormat::Jpeg)
            .unwrap();
        path
    }

    fn dimensions(path: &std::path::Path) -> (u32, u32) {
        let image = image::open(path).unwrap();
        (image.width(), image.height())
    }

    #[test]
    fn constrains_the_long_side_and_keeps_the_aspect_ratio() {
        let directory = tempfile::tempdir().unwrap();
        let source = write_jpeg(directory.path(), 1920, 1280);
        let output = directory.path().join("nested/grid.jpg");

        generate_small_thumbnail_from(&source, &output).unwrap();

        assert_eq!(dimensions(&output), (GRID_LONG_SIDE, 512));
    }

    #[test]
    fn constrains_a_portrait_source_by_its_long_side() {
        let directory = tempfile::tempdir().unwrap();
        let source = write_jpeg(directory.path(), 1280, 1920);
        let output = directory.path().join("grid.jpg");

        generate_small_thumbnail_from(&source, &output).unwrap();

        assert_eq!(dimensions(&output), (512, GRID_LONG_SIDE));
    }

    #[test]
    fn does_not_upscale_a_source_that_already_fits() {
        let directory = tempfile::tempdir().unwrap();
        let source = write_jpeg(directory.path(), 600, 400);
        let output = directory.path().join("grid.jpg");

        generate_small_thumbnail_from(&source, &output).unwrap();

        assert_eq!(dimensions(&output), (600, 400));
    }

    #[test]
    fn is_smaller_on_disk_than_the_viewer_artifact() {
        let directory = tempfile::tempdir().unwrap();
        let source = write_jpeg(directory.path(), 1920, 1280);
        let output = directory.path().join("grid.jpg");

        generate_small_thumbnail_from(&source, &output).unwrap();

        let viewer = std::fs::metadata(&source).unwrap().len();
        let grid = std::fs::metadata(&output).unwrap().len();
        assert!(
            grid < viewer,
            "grid {grid} not smaller than viewer {viewer}"
        );
    }

    #[test]
    fn reports_an_undecodable_source_rather_than_writing_an_artifact() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("viewer.jpg");
        std::fs::write(&source, b"not a jpeg").unwrap();
        let output = directory.path().join("grid.jpg");

        assert!(generate_small_thumbnail_from(&source, &output).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn backfill_preserves_viewer_and_skips_existing_grid() {
        let directory = tempfile::tempdir().unwrap();
        let source = write_jpeg(directory.path(), 1200, 800);
        let viewer_bytes = std::fs::read(&source).unwrap();
        let output = directory.path().join("grid/hash-v7.jpg");
        assert!(backfill::backfill_small_thumbnail(&source, &output, "first").unwrap());
        assert_eq!(dimensions(&output), (768, 512));
        assert_eq!(std::fs::read(&source).unwrap(), viewer_bytes);
        let grid_bytes = std::fs::read(&output).unwrap();
        std::fs::remove_file(&source).unwrap();
        assert!(!backfill::backfill_small_thumbnail(&source, &output, "again").unwrap());
        assert_eq!(std::fs::read(&output).unwrap(), grid_bytes);
        assert_eq!(
            std::fs::read_dir(output.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn failed_backfill_publishes_nothing_and_can_resume() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("viewer.jpg");
        let output = directory.path().join("grid.jpg");
        std::fs::write(&source, b"invalid JPEG").unwrap();
        assert!(backfill::backfill_small_thumbnail(&source, &output, "broken").is_err());
        assert!(!output.exists());
        write_jpeg(directory.path(), 600, 400);
        assert!(backfill::backfill_small_thumbnail(&source, &output, "retry").unwrap());
        assert_eq!(dimensions(&output), (600, 400));
    }
}
