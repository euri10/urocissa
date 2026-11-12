use super::{DataStore, v6::V6AbstractData};
use crate::public::structure::{
    abstract_data::AbstractData,
    image::{ImageCombined, ImageMetadata},
    object::{ObjectSchema, ObjectType},
};
use redb::{Database, ReadableDatabase, TableDefinition};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Stop(Child);
impl Drop for Stop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn rejections_and_confirmations_are_atomic_and_previous_v6_reader_ignores_new_table()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authored.redb");
    let store = DataStore::initialize_empty(&path)?;
    let id = arrayvec::ArrayString::from("photo")?;
    let mut photo = AbstractData::Image(ImageCombined {
        object: ObjectSchema::new(id, ObjectType::Image),
        metadata: ImageMetadata::new(id, 10, 32, 24, "jpg".into()),
    });
    photo.tag_mut().insert("cat".into());
    store.write(|writer| {
        writer.insert(&photo)?;
        writer.reject_auto_tag("photo", "dog")
    })?;
    let failed: anyhow::Result<()> = store.write(|writer| {
        writer.reject_auto_tag("photo", "cat")?;
        let mut data = photo.clone();
        data.tag_mut().clear();
        writer.insert(&data)?;
        anyhow::bail!("abort both authored writes")
    });
    assert!(failed.is_err());
    assert_eq!(store.auto_tag_rejections_for("photo")?, vec!["dog"]);
    drop(store);
    // Exact table definition/type used by the pre-feature binary (audience rollback precedent).
    let previous = Database::open(&path)?;
    let txn = previous.begin_read()?;
    let table = txn.open_table(TableDefinition::<&str, V6AbstractData>::new("records"))?;
    assert_eq!(
        table.get("photo")?.unwrap().value(),
        V6AbstractData::from(photo.clone())
    );
    drop(table);
    drop(txn);
    drop(previous);
    let restarted = DataStore::open(&path)?;
    assert_eq!(restarted.auto_tag_rejections_for("photo")?, vec!["dog"]);
    assert_eq!(
        restarted
            .reader()?
            .get("photo")?
            .unwrap()
            .into_value()
            .tag(),
        photo.tag()
    );
    Ok(())
}

/// Run with `UROCISSA_PREVIOUS_BINARY` pointing at a build of `production/reconciled`.
#[test]
fn previous_binary_serves_confirmed_user_tags_and_preserves_rejections() -> anyhow::Result<()> {
    let Some(binary) = std::env::var_os("UROCISSA_PREVIOUS_BINARY") else {
        return Ok(());
    };
    let dir = tempfile::tempdir()?;
    let backend = dir.path().join("gallery-backend");
    std::fs::create_dir_all(dir.path().join("gallery-frontend/dist/assets"))?;
    let path = backend.join("db/index_v6.redb");
    let store = DataStore::initialize_empty(&path)?;
    let id = arrayvec::ArrayString::from(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )?;
    let mut photo = AbstractData::Image(ImageCombined {
        object: ObjectSchema::new(id, ObjectType::Image),
        metadata: ImageMetadata::new(id, 10, 32, 24, "jpg".into()),
    });
    photo.tag_mut().insert("cat".into());
    store.write(|writer| {
        writer.insert(&photo)?;
        writer.reject_auto_tag(id.as_str(), "dog")
    })?;
    store.set_photo_audience(
        id.as_str(),
        crate::public::structure::audience::Audience::Public,
    )?;
    drop(store);
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let mut config = crate::public::structure::config::AppConfig::default();
    config.public.address = "127.0.0.1".into();
    config.public.port = port;
    config.public.read_only_mode = true;
    config.private.password = Some("rollback-fixture-only".into());
    std::fs::write(backend.join("config.json"), serde_json::to_vec(&config)?)?;
    let log_path = dir.path().join("previous.log");
    let log = std::fs::File::create(&log_path)?;
    let mut child = Stop(
        Command::new(binary)
            .current_dir(&backend)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("UROCISSA_PUBLIC_GALLERY", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?,
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let response = loop {
        let last = match client
            .get(format!("http://127.0.0.1:{port}/get/resource/{id}"))
            .send()
        {
            Ok(response) if response.status().is_success() => {
                break response.json::<serde_json::Value>()?;
            }
            Ok(response) => response.status().to_string(),
            Err(error) => error.to_string(),
        };
        anyhow::ensure!(
            child.0.try_wait()?.is_none(),
            "previous binary exited before readiness"
        );
        if Instant::now() >= deadline {
            anyhow::bail!(
                "previous binary did not serve the fixture ({last}); logs retained at {}",
                dir.keep().display()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        response["data"]["abstractData"]["tags"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("cat"))
    );
    assert!(!response.to_string().contains("autoTags"));
    drop(child);
    let store = DataStore::open(&path)?;
    assert_eq!(store.auto_tag_rejections_for(id.as_str())?, vec!["dog"]);
    Ok(())
}
