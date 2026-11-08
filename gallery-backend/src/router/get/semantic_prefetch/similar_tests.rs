use super::*;

/// Exercise the same HTTP guard and compact snapshots as text search, without inference.
pub(super) fn check(client: &Client, fake: &Fake) {
    let request = |query: &str, body: &str| {
        client
            .post(format!("/get/prefetch?{query}"))
            .header(ContentType::JSON)
            .body(body)
    };
    let owner = || Cookie::new("jwt", Claims::new_admin().encode());
    let source = format!("{:064x}", 2);
    let body = format!(r#"{{"Similar":"{source}"}}"#);
    // Stored vectors are sufficient even if the sidecar is offline.
    fake.0.store(false, Ordering::Relaxed);
    let response = request("sort=relevance", &body).cookie(owner()).dispatch();
    assert_eq!(response.status(), Status::Ok);
    let result: PrefetchReturn = response.into_json().unwrap();
    assert_eq!(ids(&result), vec![format!("{:064x}", 1)]);
    let selection = result.semantic_search.unwrap().selection;
    for sort in ["ascending", "descending", "relevance"] {
        let response = request(
            &format!("sort={sort}&semantic_selection={selection}"),
            &body,
        )
        .cookie(owner())
        .dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(
            ids(&response.into_json().unwrap()),
            vec![format!("{:064x}", 1)]
        );
    }
    for number in [3, 4] {
        // Stale thumbnail / not embedded yet.
        let response = request(
            "sort=relevance",
            &format!(r#"{{"Similar":"{number:064x}"}}"#),
        )
        .cookie(owner())
        .dispatch();
        assert_eq!(response.status(), Status::Conflict);
        assert!(
            response
                .into_string()
                .unwrap()
                .contains("not processed yet")
        );
    }
    assert_eq!(
        request("sort=relevance", &format!(r#"{{"Similar":"{:064x}"}}"#, 99))
            .cookie(owner())
            .dispatch()
            .status(),
        Status::NotFound
    );
    for expression in [
        r#"{"Similar":"bad-id"}"#.to_owned(),
        format!(r#"{{"Not":{body}}}"#),
        format!(r#"{{"And":[{body},{{"Semantic":"bird"}}]}}"#),
    ] {
        assert_eq!(
            request("sort=relevance", &expression)
                .cookie(owner())
                .dispatch()
                .status(),
            Status::BadRequest
        );
    }
    for expression in [&body, &format!(r#"{{"Not":{body}}}"#)] {
        // Guard all forms, not just relevance sorting.
        assert_eq!(
            request("sort=descending", expression).dispatch().status(),
            Status::Forbidden
        );
        let share = Claims {
            role: crate::router::claims::Role::Share(Box::new(ResolvedShare {
                album_id: ArrayString::from(&"a".repeat(64)).unwrap(),
                album_title: None,
                share: Share::default(),
            })),
            ..Claims::new_admin()
        }
        .encode();
        assert_eq!(
            request("sort=ascending", expression)
                .cookie(Cookie::new("jwt", share))
                .dispatch()
                .status(),
            Status::Forbidden
        );
    }
    let filtered = format!(r#"{{"And":[{body},{{"Tag":"missing"}}]}}"#);
    assert_eq!(
        request("sort=relevance", &filtered)
            .cookie(owner())
            .dispatch()
            .into_json::<PrefetchReturn>()
            .unwrap()
            .prefetch
            .data_length,
        0
    );
    fake.0.store(true, Ordering::Relaxed);
}

pub(super) fn ordering_and_selection(client: &Client, cache: &EmbeddingStore) {
    // Source #2 is not the best candidate ID; ties must not mask ordering mistakes.
    for (n, cosine) in [(1, 0.9_f32), (2, 1.0), (3, 0.99), (4, 0.5)] {
        cache
            .put(
                &format!("{n:064x}"),
                "test-model",
                0,
                &StoredEmbedding {
                    thumbnail_version: if n == 3 { 7 } else { 0 },
                    vector: vec![cosine, (1.0 - cosine * cosine).sqrt()],
                },
            )
            .unwrap();
    }
    let body = format!(r#"{{"Similar":"{:064x}"}}"#, 2);
    let request = |query: &str| {
        client
            .post(format!("/get/prefetch?{query}"))
            .header(ContentType::JSON)
            .cookie(Cookie::new("jwt", Claims::new_admin().encode()))
            .body(body.clone())
    };
    let first: PrefetchReturn = request("sort=relevance").dispatch().into_json().unwrap();
    let expected = vec![format!("{:064x}", 3), format!("{:064x}", 1)];
    assert_eq!(ids(&first), expected);
    let selection = &first.semantic_search.as_ref().unwrap().selection;
    for sort in ["descending", "ascending", "relevance"] {
        let result: PrefetchReturn =
            request(&format!("sort={sort}&semantic_selection={selection}"))
                .dispatch()
                .into_json()
                .unwrap();
        let expected = if sort == "descending" {
            expected.iter().cloned().rev().collect()
        } else {
            expected.clone()
        };
        assert_eq!(ids(&result), expected);
        // Existing bulk operations resolve precisely this snapshot, never the source or cutoff tail.
        let resolved = crate::router::selection::resolve_selection(
            result.prefetch.timestamp,
            crate::router::selection::SelectionDescriptor::AllExcept {
                excluded_indices: vec![],
            },
        )
        .unwrap();
        assert_eq!(resolved.len, 2);
        let tree = TREE.state.read().unwrap();
        let mut selected: Vec<_> = resolved
            .targets
            .iter()
            .map(|slot| tree.get(slot).unwrap().id.to_string())
            .collect();
        selected.sort();
        assert_eq!(selected, vec![format!("{:064x}", 1), format!("{:064x}", 3)]);
    }
    // New embeddings do not change an existing selection while sorting.
    cache
        .put(
            &format!("{:064x}", 4),
            "test-model",
            0,
            &StoredEmbedding {
                thumbnail_version: 0,
                vector: vec![1.0, 0.0],
            },
        )
        .unwrap();
    let resumed: PrefetchReturn =
        request(&format!("sort=relevance&semantic_selection={selection}"))
            .dispatch()
            .into_json()
            .unwrap();
    assert_eq!(ids(&resumed), expected);
    let fresh: PrefetchReturn = request("sort=relevance").dispatch().into_json().unwrap();
    assert_eq!(
        ids(&fresh),
        vec![
            format!("{:064x}", 4),
            format!("{:064x}", 3),
            format!("{:064x}", 1)
        ]
    );
}
