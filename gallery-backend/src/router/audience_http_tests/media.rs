use super::*;
use crate::router::fairing::auth_utils::my_decode_token;

fn shared(request: LocalRequest<'_>) -> LocalRequest<'_> {
    request
        .header(Header::new("x-album-id", ALBUM))
        .header(Header::new("x-share-id", "fixture-share"))
}

async fn set_download(client: &Client, enabled: bool) {
    let mut share = TREE.state.read().unwrap().albums[&id(ALBUM)]
        .metadata
        .share_list[&id("fixture-share")]
        .clone();
    share.url = id("fixture-share");
    share.show_download = enabled;
    assert_eq!(
        owner(
            client.put("/put/edit_share").json(&serde_json::json!({
                "albumId": ALBUM,
                "share": share,
            })),
            true,
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
}

fn capability(token: &str) -> ClaimsHash {
    my_decode_token(
        token,
        &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
    )
    .unwrap()
}

async fn renew(client: &Client, resource: &Value, shared_viewer: bool) -> String {
    let mut expired = capability(resource["data"]["token"].as_str().unwrap());
    expired.exp = 1;
    let request = client
        .post("/post/renew-hash-token")
        .header(Header::new(
            "Authorization",
            format!("Bearer {}", resource["token"].as_str().unwrap()),
        ))
        .json(&serde_json::json!({"expiredHashToken": expired.encode()}));
    let response = if shared_viewer {
        shared(request)
    } else {
        owner(request, true)
    }
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned()
}

pub(super) async fn download_revocation(client: &Client, records: &[AbstractData]) {
    set_download(client, true).await;
    let mut resources = Vec::new();
    for hash in [PRIVATE, PRIVATE_VIDEO] {
        let response = shared(client.get(format!("/get/resource/{hash}")))
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        let resource = response.into_json::<Value>().await.unwrap();
        assert!(!resource["data"].to_string().contains("private-gps-marker"));
        assert!(resource["data"]["abstractData"].get("alias").is_none());
        assert!(capability(resource["data"]["token"].as_str().unwrap()).allow_original);
        assert!(capability(&renew(client, &resource, true).await).allow_original);
        resources.push(resource);
    }

    for enabled in [true, false] {
        set_download(client, enabled).await;
        for resource in &resources {
            let hash = resource["data"]["abstractData"]["id"].as_str().unwrap();
            let data = records
                .iter()
                .find(|data| data.hash().as_str() == hash)
                .unwrap();
            let token = resource["data"]["token"].as_str().unwrap();
            for path in [
                data.imported_path(),
                data.compressed_path(),
                data.thumbnail_path_for_version(7),
                data.small_thumbnail_path_for_version(7),
            ] {
                let path = path
                    .strip_prefix(crate::public::constant::storage::get_data_path())
                    .unwrap();
                let expected = if !enabled && path.starts_with("object/imported") {
                    Status::Forbidden
                } else {
                    Status::Ok
                };
                // Browsers send share credentials either in headers or download URLs.
                for query_auth in [false, true] {
                    let url = format!("/{}?token={token}", path.display());
                    let request = if query_auth {
                        client.get(format!("{url}&albumId={ALBUM}&shareId=fixture-share"))
                    } else {
                        shared(client.get(url))
                    };
                    assert_eq!(
                        request.dispatch().await.status(),
                        expected,
                        "{}",
                        path.display()
                    );
                }
                assert_eq!(
                    owner(
                        client.get(format!("/{}?token={token}", path.display())),
                        true
                    )
                    .dispatch()
                    .await
                    .status(),
                    Status::Ok,
                    "owner {}",
                    path.display()
                );
            }
        }
    }

    for resource in &resources {
        let renewed = renew(client, resource, true).await;
        assert!(
            !capability(&renewed).allow_original,
            "renewal retained revoked downloads"
        );
        // Re-enabling downloads cannot promote a preview-only capability.
        let mut preview_resource = resource.clone();
        preview_resource["data"]["token"] = Value::String(renewed);
        set_download(client, true).await;
        assert!(!capability(&renew(client, &preview_resource, true).await).allow_original);
        set_download(client, false).await;
    }

    let owner_resource = owner(client.get(format!("/get/resource/{PRIVATE}")), true)
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    assert!(capability(&renew(client, &owner_resource, false).await).allow_original);
}

#[test]
fn share_download_revocation_in_public_gallery_http() {
    isolated(true, true);
}

#[test]
fn share_download_revocation_in_private_gallery_http() {
    isolated(false, true);
}
