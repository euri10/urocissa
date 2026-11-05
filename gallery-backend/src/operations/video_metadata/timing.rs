//! Remux timing options derived from the source, never from rounded seconds.
use anyhow::{Context, Result, ensure};
use serde_json::Value;

// FFmpeg's expression evaluator uses doubles; refuse inexact integers.
const EXACT_INTEGER: i64 = 1_i64 << 53;

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub(super) fn copy_options(probe: &Value) -> Result<Vec<String>> {
    let streams = probe["streams"].as_array().context("missing streams")?;
    ensure!(!streams.is_empty(), "missing streams");
    let mut timescale = 1000_u32;
    let mut options = Vec::new();
    for (index, stream) in streams.iter().enumerate() {
        let time_base = stream["time_base"].as_str().context("missing time base")?;
        let (numerator, denominator) = time_base.split_once('/').context("invalid time base")?;
        let numerator: u32 = numerator.parse().context("invalid time base")?;
        let denominator: u32 = denominator.parse().context("invalid time base")?;
        ensure!(numerator > 0 && denominator > 0, "invalid time base");
        let denominator = denominator / gcd(numerator, denominator);
        // MP4 edit-list starts use the movie clock. Its default 1 ms grid can
        // move a fractional stream start; the LCM represents every input tick.
        timescale = timescale
            .checked_mul(denominator / gcd(timescale, denominator))
            .filter(|scale| i32::try_from(*scale).is_ok())
            .context("source time bases exceed MP4 movie timescale")?;

        if stream["codec_type"] != "audio" || stream["codec_name"] != "aac" {
            continue;
        }
        let sample_rate: u32 = stream["sample_rate"]
            .as_str()
            .context("missing AAC sample rate")?
            .parse()
            .context("invalid AAC sample rate")?;
        ensure!(
            sample_rate > 0 && time_base == format!("1/{sample_rate}"),
            "unsupported AAC time base"
        );
        let start = stream["start_pts"].as_i64().context("missing AAC start")?;
        let duration = stream["duration_ts"]
            .as_i64()
            .context("missing AAC duration")?;
        ensure!(duration > 0, "invalid AAC duration");
        let end = start
            .checked_add(duration)
            .context("AAC endpoint overflow")?;
        ensure!(
            (-EXACT_INTEGER..=EXACT_INTEGER).contains(&start)
                && duration <= EXACT_INTEGER
                && (-EXACT_INTEGER..=EXACT_INTEGER).contains(&end),
            "AAC timing exceeds exact expression range"
        );
        // AAC stream copy can expand a final partial packet to a full frame.
        // Keep its original endpoint and preserve both timestamps explicitly.
        // The independent packet gate must still prove exact source equality.
        options.push(format!("-bsf:{index}"));
        options.push(format!(
            "setts=pts=PTS:dts=DTS:duration=min(DURATION\\,{end}-PTS)"
        ));
    }
    options.extend(["-movie_timescale".to_owned(), timescale.to_string()]);
    Ok(options)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::copy_options;

    fn source() -> serde_json::Value {
        json!({"streams": [
            {"codec_type": "video", "codec_name": "h264", "time_base": "1/16384"},
            {"codec_type": "audio", "codec_name": "aac", "time_base": "1/48000",
             "sample_rate": "48000", "start_pts": 94944, "duration_ts": 25024}
        ]})
    }

    #[test]
    fn derives_exact_movie_clock_and_audio_endpoint() {
        assert_eq!(
            copy_options(&source()).unwrap(),
            [
                "-bsf:1",
                "setts=pts=PTS:dts=DTS:duration=min(DURATION\\,119968-PTS)",
                "-movie_timescale",
                "6144000"
            ]
        );
        let mut source = source();
        source["streams"][1]["start_pts"] = json!(-1024);
        assert!(copy_options(&source).unwrap()[1].contains("24000-PTS"));
    }

    #[test]
    fn refuses_unknown_inexact_or_overflowing_timing() {
        for (field, value) in [
            ("time_base", json!("0/48000")),
            ("time_base", json!("1/0")),
            ("time_base", json!("invalid")),
            ("time_base", json!("1/47999")),
            ("sample_rate", json!("0")),
            ("start_pts", json!(null)),
            ("start_pts", json!(i64::MAX)),
            ("start_pts", json!(1_i64 << 54)),
            ("duration_ts", json!(null)),
            ("duration_ts", json!(0)),
            ("duration_ts", json!(-1)),
            ("duration_ts", json!(i64::MAX)),
        ] {
            let mut probe = source();
            probe["streams"][1][field] = value;
            assert!(copy_options(&probe).is_err(), "accepted invalid {field}");
        }
        let mut probe = source();
        probe["streams"][0]["time_base"] = json!("1/2147483647");
        assert!(copy_options(&probe).is_err());
        assert!(copy_options(&json!({"streams": []})).is_err());
    }
}
