use forme_protocol as p;
fn query() -> p::ToolArtifactCatalogQuery {
    p::ToolArtifactCatalogQuery {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: p::SessionId("session:image".into()),
        run: p::RunId("run:image".into()),
        page: p::DataPageQuery {
            schema_version: p::SchemaVersion(1),
            offset: 0,
            limit: 1,
            snapshot: None,
        },
    }
}
fn read() -> p::ToolArtifactRead {
    p::ToolArtifactRead {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: query().session,
        run: query().run,
        content_ref: p::ContentRef(format!("content:{}", p::sha256_content_digest(b"object").0)),
        content_digest: p::sha256_content_digest(b"image"),
        offset: 0,
        max_bytes: 4,
    }
}
#[test]
fn tool_image_versions_fields_pagination_and_chunk_boundaries_are_closed() {
    query().validate().unwrap();
    read().validate().unwrap();
    let mut old = query();
    old.schema_version = p::SchemaVersion(1);
    assert!(old.validate().is_err());
    let mut unbound = query();
    unbound.page.offset = 1;
    assert!(unbound.validate().is_err());
    for field in ["owner", "path", "job", "grant"] {
        let mut value = serde_json::to_value(read()).unwrap();
        value[field] = serde_json::json!("forged");
        assert!(serde_json::from_value::<p::ToolArtifactRead>(value).is_err());
    }
    for max in [0, p::TOOL_ARTIFACT_CHUNK_BYTES + 1] {
        let mut request = read();
        request.max_bytes = max;
        assert!(request.validate().is_err());
    }
    let r = read();
    let chunk = p::ToolArtifactChunk {
        schema_version: p::TOOL_ARTIFACT_SCHEMA_VERSION,
        session: r.session.clone(),
        run: r.run.clone(),
        content_ref: r.content_ref.clone(),
        content_digest: r.content_digest.clone(),
        offset: 0,
        next_offset: 4,
        total_bytes: 4,
        finished: true,
        bytes: b"test".to_vec(),
    };
    chunk.validate_for(&r).unwrap();
    for case in 0..7 {
        let mut changed = chunk.clone();
        match case {
            0 => changed.run = p::RunId("run:other".into()),
            1 => changed.session = p::SessionId("session:other".into()),
            2 => changed.content_digest = p::sha256_content_digest(b"other"),
            3 => changed.next_offset = 3,
            4 => changed.total_bytes = 3,
            5 => changed.finished = false,
            _ => changed.bytes.push(0),
        };
        assert!(changed.validate_for(&r).is_err());
    }
}
