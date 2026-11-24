use super::*;
use crate::public::structure::{
    abstract_data::AbstractData,
    image::{ImageCombined, ImageMetadata},
    object::{ObjectSchema, ObjectType},
};
use std::{
    process::Command,
    time::{Duration, Instant},
};

fn docker(args: &[&str]) -> Result<String> {
    let output = Command::new("docker")
        .args(["--host", "unix:///var/run/docker.sock"])
        .args(args)
        .output()?;
    anyhow::ensure!(output.status.success(), "Disposable Docker command failed");
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
struct Container(String);
impl Drop for Container {
    fn drop(&mut self) {
        let _ = docker(&["stop", "--time", "20", &self.0]);
        let _ = docker(&["rm", &self.0]);
    }
}

pub(crate) fn round() -> ReviewRound {
    ReviewRound {
        id: "fixture-round".into(),
        kind: "random".into(),
        captured_at: 1,
        model: "fixture-model".into(),
        vocabulary: "fixture-vocabulary".into(),
        materialized_at: 1,
        library_revision: 1,
        population: 100,
        plant: String::new(),
        separation_confirmed: false,
        after_captured_at: None,
        photos: vec![ReviewPhoto {
            hash: "0".repeat(64),
            version: 0,
            timestamp: 1,
            role: "random".into(),
            enabled: true,
            pending: false,
            before: vec![Suggestion {
                concept: "plant".into(),
                label: "plant".into(),
                judgment: None,
            }],
            after: None,
            seen_before: false,
            seen_after: false,
        }],
        feedback: String::new(),
        effort_seconds: 0,
    }
}

#[test]
fn judgments_survive_reopen_without_touching_records_or_feedback() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("index_v6.redb");
    {
        let store = DataStore::initialize_empty(&path)?;
        assert!(store.acceptance_round("random")?.is_none());
        store.update_acceptance("random", |_| Ok(round()))?;
        store.update_acceptance("random", |previous| {
            let mut round = previous.unwrap();
            round.photos[0].before[0].judgment = Some(Judgment::No);
            round.photos[0].seen_before = true;
            Ok(round)
        })?;
        assert_eq!(store.record_count()?, 0);
        assert!(store.auto_tag_rejections()?.is_empty());
        assert!(store.auto_tag_settings()?.enabled(&"0".repeat(64))?);
    }
    let store = DataStore::open(&path)?;
    let round = store.acceptance_round("random")?.unwrap();
    assert_eq!(round.photos[0].before[0].judgment, Some(Judgment::No));
    assert!(round.photos[0].seen_before);
    // The old reader only opens the frozen V6 records table and ignores additions.
    assert_eq!(store.reader()?.len()?, 0);
    Ok(())
}

/// Opt-in actual previous executable, on generated disposable data only.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One end-to-end fixture owns its generated data and exact container cleanup"
)]
fn previous_binary_ignores_additive_review_table() -> Result<()> {
    let Ok(image_id) = std::env::var("UROCISSA_ROLLBACK_IMAGE") else {
        return Ok(());
    };
    ensure_image_id(&image_id)?;
    let root = tempfile::tempdir()?;
    let data = root.path().join("urocissa");
    std::fs::create_dir_all(data.join("tmp"))?;
    let hash = "1".repeat(64);
    let originals = data.join("object/imported/11");
    std::fs::create_dir_all(&originals)?;
    let original = originals.join(format!("{hash}.jpg"));
    image::RgbImage::new(32, 24).save(&original)?;
    let bytes = std::fs::read(&original)?;
    let fixture_id = arrayvec::ArrayString::<64>::from(hash.as_str())
        .map_err(|_| anyhow::anyhow!("Invalid fixture ID"))?;
    let mut object = ObjectSchema::new(fixture_id, ObjectType::Image);
    object.tags.insert("confirmed-owner-tag".into());
    let record = AbstractData::Image(ImageCombined {
        metadata: ImageMetadata::new(object.id, bytes.len() as u64, 32, 24, "jpg".into()),
        object,
    });
    let path = data.join("db/index_v6.redb");
    {
        let store = DataStore::initialize_empty(&path)?;
        store.write(|writer| -> Result<()> {
            writer.insert(&record)?;
            Ok(())
        })?;
        store.migrate_photo_audience()?;
        store.migrate_album_audience()?;
        store.update_acceptance("random", |_| Ok(round()))?;
        let write = store.db.begin_write()?;
        {
            write
                .open_table(super::super::auto_tags::REJECTIONS)?
                .insert((hash.as_str(), "plant"), 1)?;
        }
        write.commit()?;
    }
    drop(crate::storage::embeddings::EmbeddingStore::open(
        &data.join("db/embeddings.redb"),
    )?);
    let password = "acceptance-disposable-rollback-fixture"; // betterleaks:allow -- generated disposable database only
    std::fs::write(
        data.join("config.json"),
        serde_json::to_vec(&serde_json::json!({
        "public":{"address":"0.0.0.0","port":5673,"syncPaths":[],"readOnlyMode":false,
            "disableImg":false,"limits":{"file":"512MiB","json":"10MiB","data-form":"1GiB"}},
        "private":{"password":password,"authKey":null,"discordHookUrl":null} // betterleaks:allow -- same generated disposable password
        }))?,
    )?;
    let mount = format!("type=bind,src={},dst=/data", root.path().display());
    let id = docker(&[
        "create",
        "--pull",
        "never",
        "--read-only",
        "--user",
        "1000:1000",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges:true",
        "--cpus",
        "1",
        "--memory",
        "1g",
        "--stop-signal",
        "SIGINT",
        "--publish",
        "127.0.0.1::5673",
        "--mount",
        &mount,
        "--tmpfs",
        "/tmp:rw,size=64m",
        "--env",
        "XDG_DATA_HOME=/data",
        "--env",
        "TMPDIR=/data/urocissa/tmp",
        "--env",
        "UROCISSA_PUBLIC_GALLERY=1",
        "--workdir",
        "/app/gallery-backend",
        "--entrypoint",
        "/app/gallery-backend/urocissa",
        &image_id,
    ])?;
    let container = Container(id);
    docker(&["start", &container.0])?;
    let port = docker(&["port", &container.0, "5673/tcp"])?;
    let url = format!("http://{port}");
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if client
            .post(format!("{url}/post/authenticate"))
            .json("")
            .send()
            .is_ok_and(|response| response.status().as_u16() == 401)
        {
            break;
        }
        if Instant::now() >= deadline {
            // Only this generated fixture; production is never mounted here.
            let logs = Command::new("docker")
                .args([
                    "--host",
                    "unix:///var/run/docker.sock",
                    "logs",
                    &container.0,
                ])
                .output()?;
            eprintln!(
                "Disposable fixture startup: {} {}",
                String::from_utf8_lossy(&logs.stdout),
                String::from_utf8_lossy(&logs.stderr)
            );
            anyhow::bail!("Previous executable not ready");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let owner: String = client
        .post(format!("{url}/post/authenticate"))
        .json(password)
        .send()?
        .error_for_status()?
        .json()?;
    let resource: serde_json::Value = client
        .get(format!("{url}/get/resource/{hash}"))
        .header("Cookie", format!("jwt={owner}"))
        .send()?
        .error_for_status()?
        .json()?;
    let record = &resource["data"]["abstractData"];
    assert_eq!(record["tags"], serde_json::json!(["confirmed-owner-tag"]));
    let token = resource["data"]["token"].as_str().unwrap();
    let read = client
        .get(format!("{url}/object/imported/11/{hash}.jpg"))
        .bearer_auth(token)
        .header("Cookie", format!("jwt={owner}"))
        .send()?
        .error_for_status()?
        .bytes()?;
    assert_eq!(&read[..], bytes);
    drop(container);
    let store = DataStore::open(&path)?;
    assert!(store.acceptance_round("random")?.is_some());
    assert_eq!(store.auto_tag_rejections_for(&hash)?, vec!["plant"]);
    assert!(
        store
            .reader()?
            .get(&hash)?
            .unwrap()
            .into_value()
            .tag()
            .contains("confirmed-owner-tag")
    );
    Ok(())
}

fn ensure_image_id(value: &str) -> Result<()> {
    anyhow::ensure!(
        value.len() == 71
            && value.starts_with("sha256:")
            && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Use an exact local image ID"
    );
    Ok(())
}
