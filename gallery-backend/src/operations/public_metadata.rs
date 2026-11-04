//! Public presentation policy. Stored records and owner responses keep their metadata.
use chrono::{Datelike, TimeZone, Utc};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerKind {
    Owner,
    Anonymous,
    Share,
}

impl ViewerKind {
    pub fn for_response(is_owner: bool, is_share: bool) -> Self {
        if is_share {
            Self::Share
        } else if is_owner {
            Self::Owner
        } else {
            Self::Anonymous
        }
    }
}

/// Keep only the month used by the gallery timeline, never a capture day/time.
pub fn presentation_timestamp(viewer: ViewerKind, timestamp: i64) -> i64 {
    if viewer == ViewerKind::Owner {
        return timestamp;
    }
    Utc.timestamp_millis_opt(timestamp)
        .single()
        .and_then(|date| {
            Utc.with_ymd_and_hms(date.year(), date.month(), 1, 0, 0, 0)
                .single()
        })
        .map_or(0, |date| date.timestamp_millis())
}

/// Redact a serialized item record for anonymous or share visitors.
///
/// Gallery identity, rendering, flags, user descriptions and collection membership
/// remain part of the gallery protocol. Media metadata is limited to dimensions,
/// size, camera/lens, exposure and video duration/codec. Both lists are explicit:
/// new record fields and unknown EXIF/ffprobe keys fail closed.
pub fn redact_record(viewer: ViewerKind, mut record: Value) -> Value {
    if viewer == ViewerKind::Owner {
        return record;
    }
    let Some(fields) = record.as_object_mut() else {
        return Value::Null;
    };
    fields.retain(|key, _| {
        matches!(
            key.as_str(),
            "type"
                | "id"
                | "objType"
                | "pending"
                | "thumbhash"
                | "cacheVersion"
                | "description"
                | "tags"
                | "isFavorite"
                | "isArchived"
                | "isTrashed"
                | "albums"
                | "ext"
                | "size"
                | "width"
                | "height"
                | "duration"
                | "exifVec"
                | "title"
                | "cover"
                | "itemCount"
                | "itemSize"
                | "startTime"
                | "endTime"
                | "lastModifiedTime"
        )
    });
    if let Some(exif) = fields.get_mut("exifVec") {
        if let Some(exif) = exif.as_object_mut() {
            exif.retain(|key, _| {
                matches!(
                    key.as_str(),
                    "Make"
                        | "Model"
                        | "LensMake"
                        | "LensModel"
                        | "LensSpecification"
                        | "FNumber"
                        | "ApertureValue"
                        | "ExposureTime"
                        | "ShutterSpeedValue"
                        | "PhotographicSensitivity"
                        | "ISOSpeed"
                        | "ISOSpeedRatings"
                        | "FocalLength"
                        | "FocalLengthIn35mmFilm"
                        | "ImageWidth"
                        | "ImageLength"
                        | "PixelXDimension"
                        | "PixelYDimension"
                        | "width"
                        | "height"
                        | "size"
                        | "duration"
                        | "codec_name"
                        | "codec_long_name"
                        | "codec_tag"
                        | "codec_tag_string"
                )
            });
        } else {
            *exif = serde_json::json!({});
        }
    }
    // Album ranges are displayed as exact dates by existing clients. Withhold them;
    // the separate scrollbar already carries the year/month boundaries.
    for key in ["startTime", "endTime"] {
        if let Some(value) = fields.get_mut(key) {
            *value = Value::Null;
        }
    }
    if let Some(value) = fields.get_mut("lastModifiedTime") {
        *value = presentation_timestamp(viewer, value.as_i64().unwrap_or(0)).into();
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redaction_is_an_allow_list_for_every_non_owner_and_record_kind() {
        let safe_exif = json!({
            "Make": "Camera", "Model": "Model", "LensModel": "Lens", "FNumber": "f/2.8",
            "ExposureTime": "1/125 s", "PhotographicSensitivity": "200", "FocalLength": "35 mm",
            "width": "1920", "height": "1080", "size": "1000", "duration": "2.5",
            "codec_name": "h264", "codec_long_name": "H.264"
        });
        for kind in ["image", "video", "album"] {
            let mut exif = safe_exif.clone();
            for key in [
                "GPSLatitude",
                "GPSLongitude",
                "BodySerialNumber",
                "LensSerialNumber",
                "Artist",
                "Copyright",
                "OwnerName",
                "MakerNote",
                "Software",
                "DateTimeOriginal",
                "creation_time",
                "TAG:location",
                "filename",
                "FutureUnknownField",
            ] {
                exif[key] = json!("private marker");
            }
            let record = json!({
                "type": kind, "id": "id", "width": 1920, "height": 1080, "size": 1000,
                "duration": 2.5, "exifVec": exif, "alias": [{"file": "/private/original.jpg"}],
                "phash": [1, 2], "updateAt": 1_793_825_335_123_i64, "createdTime": 1_793_825_335_123_i64,
                "futureRecordField": {"gps": "private marker"},
                "startTime": 1_793_825_335_123_i64, "endTime": 1_793_825_335_123_i64,
                "lastModifiedTime": 1_793_825_335_123_i64, "shareList": {"test-share": {"showMetadata": true}}
            });
            assert_eq!(redact_record(ViewerKind::Owner, record.clone()), record);
            for viewer in [ViewerKind::Anonymous, ViewerKind::Share] {
                assert_eq!(
                    redact_record(viewer, record.clone()),
                    json!({
                        "type": kind, "id": "id", "width": 1920, "height": 1080,
                        "size": 1000, "duration": 2.5, "exifVec": safe_exif,
                        "startTime": null, "endTime": null,
                        "lastModifiedTime": 1_793_491_200_000_i64
                    })
                );
            }
        }
    }

    #[test]
    fn dates_are_month_only_including_negative_and_invalid_timestamps() {
        for (input, expected) in [
            (1_793_825_335_123, 1_793_491_200_000),
            (0, 0),
            (-1, -2_678_400_000),
            (i64::MAX, 0),
        ] {
            assert_eq!(presentation_timestamp(ViewerKind::Owner, input), input);
            for viewer in [ViewerKind::Anonymous, ViewerKind::Share] {
                assert_eq!(presentation_timestamp(viewer, input), expected);
            }
        }
    }

    #[test]
    fn malformed_or_missing_metadata_fails_closed() {
        for record in [Value::Null, json!([]), json!("private marker")] {
            assert_eq!(redact_record(ViewerKind::Anonymous, record), Value::Null);
        }
        assert_eq!(
            redact_record(ViewerKind::Share, json!({"exifVec": "private marker"})),
            json!({"exifVec": {}})
        );
        assert_eq!(redact_record(ViewerKind::Anonymous, json!({})), json!({}));
        assert_eq!(
            ViewerKind::for_response(false, false),
            ViewerKind::Anonymous
        );
        assert_eq!(ViewerKind::for_response(true, false), ViewerKind::Owner);
        assert_eq!(ViewerKind::for_response(true, true), ViewerKind::Share);
        assert_eq!(ViewerKind::for_response(false, true), ViewerKind::Share);
    }
}
