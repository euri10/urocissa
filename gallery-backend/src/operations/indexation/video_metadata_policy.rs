//! Shared source-metadata policy for new compression and existing derivatives.
use serde_json::Value;

pub const STRIP_SOURCE_METADATA_ARGS: [&str; 6] = [
    "-map_metadata",
    "-1",
    "-map_metadata:s",
    "-1",
    "-map_chapters",
    "-1",
];

// MP4 muxers regenerate these technical tags even when source tags are disabled.
// Keep the allow-list aligned with compressed_video_metadata_tests.rs (.4).
pub fn tags_are_clean(probe: &Value) -> bool {
    clean_tags(&probe["format"]["tags"], None)
        && probe["streams"].as_array().is_some_and(|streams| {
            streams
                .iter()
                .all(|stream| clean_tags(&stream["tags"], stream["codec_type"].as_str()))
        })
        && probe["chapters"].as_array().is_none_or(Vec::is_empty)
}

fn clean_tags(tags: &Value, stream_type: Option<&str>) -> bool {
    if tags.is_null() {
        return true;
    }
    tags.as_object().is_some_and(|tags| {
        tags.iter().all(|(key, value)| {
            let Some(value) = value.as_str() else {
                return false;
            };
            match (stream_type, key.as_str()) {
                (None, "major_brand" | "minor_version" | "compatible_brands") => true,
                (None, "encoder") => value.starts_with("Lavf"),
                (Some(_), "encoder") => value.starts_with("Lavc"),
                (Some(_), "language") => value == "und",
                (Some("video"), "handler_name") => value == "VideoHandler",
                (Some("audio"), "handler_name") => value == "SoundHandler",
                (Some(_), "vendor_id") => value == "[0][0][0][0]",
                _ => false,
            }
        })
    })
}
