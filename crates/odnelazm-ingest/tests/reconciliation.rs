use odnelazm_ingest::store::MemberRecord;
use odnelazm_ingest::{DataStore, PostgresStore};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

const LOCAL_TEST_DATABASE_URL: &str = "postgres://odnelazm:odnelazm@localhost:5432/odnelazm";

#[tokio::test]
#[ignore = "requires the explicitly configured local integration database"]
async fn reconciliation_is_idempotent_preserves_summaries_and_tracks_lifecycle() {
    let database_url = std::env::var("ODNELAZM_TEST_DATABASE_URL")
        .expect("ODNELAZM_TEST_DATABASE_URL must be explicitly set");
    assert_eq!(database_url, LOCAL_TEST_DATABASE_URL);

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .unwrap();
    let mut test_lock = pool.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(734821950)")
        .execute(&mut *test_lock)
        .await
        .unwrap();
    let store = PostgresStore::from_pool(pool.clone());
    store.migrate().await.unwrap();

    let (sitting_id, raw_json): (Uuid, serde_json::Value) = sqlx::query_as(
        r#"
        SELECT s.id, s.raw_json
        FROM sittings s
        WHERE EXISTS (
            SELECT 1
            FROM bill_mentions bm
            JOIN bill_mention_speakers bms ON bms.bill_mention_id = bm.id
            WHERE bm.sitting_id = s.id AND bms.summary IS NOT NULL
        )
        ORDER BY s.date DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let sitting: odnelazm::HansardSitting = serde_json::from_value(raw_json).unwrap();
    let speakers = odnelazm_ingest::extract::extract_speakers(&sitting);
    let bills = odnelazm_ingest::extract::extract_bills(&sitting);
    let topics = odnelazm_ingest::extract::extract_topics(&sitting);

    let summary_counts_before = summary_counts(&pool).await;
    let stale_counts_before = stale_counts(&pool).await;

    assert_eq!(
        store
            .reconcile_sitting(&sitting, "13th-parliament", &speakers, &bills, &topics)
            .await
            .unwrap()
            .sitting_id,
        sitting_id
    );
    let first_snapshot = derived_snapshot(&pool, sitting_id).await;

    store
        .reconcile_sitting(&sitting, "13th-parliament", &speakers, &bills, &topics)
        .await
        .unwrap();
    let second_snapshot = derived_snapshot(&pool, sitting_id).await;

    assert_eq!(first_snapshot, second_snapshot);
    assert_eq!(summary_counts_before, summary_counts(&pool).await);
    assert_eq!(stale_counts_before, stale_counts(&pool).await);

    changed_input_stales_preserved_summary_and_missing_rows_become_inactive().await;
}

#[tokio::test]
#[ignore = "requires the explicitly configured local integration database"]
async fn sitting_reconciliation_links_speakers_without_member_import() {
    let database_url = std::env::var("ODNELAZM_TEST_DATABASE_URL")
        .expect("ODNELAZM_TEST_DATABASE_URL must be explicitly set");
    assert_eq!(database_url, LOCAL_TEST_DATABASE_URL);

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .unwrap();
    let mut test_lock = pool.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(734821950)")
        .execute(&mut *test_lock)
        .await
        .unwrap();
    let store = PostgresStore::from_pool(pool.clone());
    store.migrate().await.unwrap();

    let marker = Uuid::new_v4();
    let slug = format!("integration-{marker}");
    let canonical_member = store
        .upsert_member(&MemberRecord {
            name: format!("Canonical Member {marker}"),
            url: format!("/mps-performance/13th-parliament/national-assembly/{slug}/"),
            source: odnelazm::DataSource::Current,
            house: "National Assembly".into(),
            parliament: "13th-parliament".into(),
            role: None,
            constituency: None,
        })
        .await
        .unwrap();
    let archive_member = store
        .upsert_member(&MemberRecord {
            name: format!("Archive Member {marker}"),
            url: format!("/person/archive-{marker}/"),
            source: odnelazm::DataSource::Archive,
            house: "National Assembly".into(),
            parliament: "13th-parliament".into(),
            role: None,
            constituency: None,
        })
        .await
        .unwrap();
    sqlx::query(
        r#"
        INSERT INTO member_sources
            (member_id, data_source_id, external_key, source_url, normalized_url)
        SELECT $1, id, $2, $3, $4
        FROM data_sources WHERE source_key = 'mzalendo-archive'
        "#,
    )
    .bind(archive_member)
    .bind(&slug)
    .bind(format!("https://info.mzalendo.com/person/{slug}/"))
    .bind(format!("https://info.mzalendo.com/person/{slug}"))
    .execute(&pool)
    .await
    .unwrap();
    let preserved_member = store
        .upsert_member(&MemberRecord {
            name: format!("Preserved Member {marker}"),
            url: format!("/person/preserved-{marker}/"),
            source: odnelazm::DataSource::Current,
            house: "National Assembly".into(),
            parliament: "13th-parliament".into(),
            role: None,
            constituency: None,
        })
        .await
        .unwrap();

    let ambiguous_name = format!("Ambiguous Integration {marker}");
    let ambiguous_members: Vec<Uuid> = sqlx::query_scalar(
        r#"
        INSERT INTO members (id, name, url, house, parliament)
        VALUES
            (uuid_generate_v4(), $1, $2, 'National Assembly', '13th-parliament'),
            (uuid_generate_v4(), $1, $3, 'National Assembly', '13th-parliament')
        RETURNING id
        "#,
    )
    .bind(&ambiguous_name)
    .bind(format!("https://example.test/{marker}/ambiguous-a"))
    .bind(format!("https://example.test/{marker}/ambiguous-b"))
    .fetch_all(&pool)
    .await
    .unwrap();

    let cross_house_name = format!("Cross House Integration {marker}");
    let scoped_members: Vec<(Uuid, String, String)> = sqlx::query_as(
        r#"
        INSERT INTO members (id, name, url, house, parliament)
        VALUES
            (uuid_generate_v4(), $1, $2, 'National Assembly', '13th-parliament'),
            (uuid_generate_v4(), $1, $3, 'Senate', '13th-parliament'),
            (uuid_generate_v4(), $1, $4, 'National Assembly', '12th-parliament')
        RETURNING id, house, parliament
        "#,
    )
    .bind(&cross_house_name)
    .bind(format!("https://example.test/{marker}/cross-house-na"))
    .bind(format!("https://example.test/{marker}/cross-house-senate"))
    .bind(format!("https://example.test/{marker}/cross-parliament-na"))
    .fetch_all(&pool)
    .await
    .unwrap();
    let national_assembly_member = scoped_members
        .iter()
        .find_map(|(id, house, parliament)| {
            (house == "National Assembly" && parliament == "13th-parliament").then_some(*id)
        })
        .unwrap();
    let previous_parliament_member = scoped_members
        .iter()
        .find_map(|(id, house, parliament)| {
            (house == "National Assembly" && parliament == "12th-parliament").then_some(*id)
        })
        .unwrap();

    let existing_name = format!("Existing Integration {marker}");
    let existing_url = format!("/person/{slug}");
    let existing_speaker: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO speakers (id, name, url, member_id, house, parliament)
        VALUES (uuid_generate_v4(), $1, $2, $3, 'National Assembly', '13th-parliament')
        RETURNING id
        "#,
    )
    .bind(&existing_name)
    .bind(&existing_url)
    .bind(preserved_member)
    .fetch_one(&pool)
    .await
    .unwrap();

    let sitting = synthetic_linkage_sitting(
        marker,
        &[
            (
                format!("URL Integration {marker}"),
                Some(existing_url.clone()),
            ),
            (ambiguous_name.clone(), None),
            (cross_house_name.clone(), None),
            (format!("No Match Integration {marker}"), None),
            (existing_name.clone(), Some(existing_url)),
        ],
    );
    let result = reconcile_extracted_result(&store, &sitting).await;
    assert_eq!(result.speakers_linked, 2);

    let linked: Option<Uuid> =
        sqlx::query_scalar("SELECT member_id FROM speakers WHERE name = $1 AND url = $2")
            .bind(format!("URL Integration {marker}"))
            .bind(format!("/person/{slug}"))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(linked, Some(canonical_member));
    assert_ne!(linked, Some(archive_member));

    let house_scoped: Option<Uuid> =
        sqlx::query_scalar("SELECT member_id FROM speakers WHERE name = $1 AND url IS NULL")
            .bind(&cross_house_name)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(house_scoped, Some(national_assembly_member));

    let previous_marker = Uuid::new_v4();
    let previous_sitting =
        synthetic_linkage_sitting(previous_marker, &[(cross_house_name.clone(), None)]);
    let previous_speakers = odnelazm_ingest::extract::extract_speakers(&previous_sitting);
    let previous_bills = odnelazm_ingest::extract::extract_bills(&previous_sitting);
    let previous_topics = odnelazm_ingest::extract::extract_topics(&previous_sitting);
    let previous_result = store
        .reconcile_sitting(
            &previous_sitting,
            "12th-parliament",
            &previous_speakers,
            &previous_bills,
            &previous_topics,
        )
        .await
        .unwrap();
    assert_eq!(previous_result.speakers_linked, 1);

    let contextual_links: Vec<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT parliament, member_id FROM speakers WHERE name = $1 ORDER BY parliament",
    )
    .bind(&cross_house_name)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        contextual_links,
        vec![
            ("12th-parliament".into(), Some(previous_parliament_member)),
            ("13th-parliament".into(), Some(national_assembly_member)),
        ]
    );

    for name in [&ambiguous_name, &format!("No Match Integration {marker}")] {
        let member_id: Option<Uuid> =
            sqlx::query_scalar("SELECT member_id FROM speakers WHERE name = $1 AND url IS NULL")
                .bind(name)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(member_id, None);
    }
    let preserved: Option<Uuid> =
        sqlx::query_scalar("SELECT member_id FROM speakers WHERE id = $1")
            .bind(existing_speaker)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(preserved, Some(preserved_member));

    sqlx::query("DELETE FROM sittings WHERE id = $1")
        .bind(result.sitting_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sittings WHERE id = $1")
        .bind(previous_result.sitting_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM speakers WHERE name LIKE $1")
        .bind(format!("%{marker}%"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM members WHERE id = ANY($1)")
        .bind(
            [
                vec![canonical_member, archive_member, preserved_member],
                [
                    ambiguous_members,
                    scoped_members.into_iter().map(|(id, _, _)| id).collect(),
                ]
                .concat(),
            ]
            .concat(),
        )
        .execute(&pool)
        .await
        .unwrap();
}

async fn changed_input_stales_preserved_summary_and_missing_rows_become_inactive() {
    let database_url = std::env::var("ODNELAZM_TEST_DATABASE_URL")
        .expect("ODNELAZM_TEST_DATABASE_URL must be explicitly set");
    assert_eq!(database_url, LOCAL_TEST_DATABASE_URL);

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .unwrap();
    let store = PostgresStore::from_pool(pool.clone());
    store.migrate().await.unwrap();

    let marker = Uuid::new_v4();
    let speaker_name = format!("Integration Speaker {marker}");
    let mut sitting = synthetic_topic_sitting(marker, &speaker_name);
    let sitting_id = reconcile_extracted(&store, &sitting).await;
    let (topic_id, speaker_id): (Uuid, Uuid) = sqlx::query_as(
        r#"
        SELECT t.id, ts.speaker_id
        FROM topics t JOIN topic_speakers ts ON ts.topic_id = t.id
        WHERE t.sitting_id = $1
        "#,
    )
    .bind(sitting_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        UPDATE topics SET
            summary = 'topic summary', summary_model = 'test-model',
            summary_input_hash = input_hash, summary_generated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(topic_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        UPDATE topic_speakers SET
            summary = 'speaker summary', summary_model = 'test-model',
            summary_input_hash = input_hash, summary_generated_at = now()
        WHERE topic_id = $1 AND speaker_id = $2
        "#,
    )
    .bind(topic_id)
    .bind(speaker_id)
    .execute(&pool)
    .await
    .unwrap();

    reconcile_extracted(&store, &sitting).await;
    let unchanged: (i32, String, Option<String>, Option<String>, bool) = sqlx::query_as(
        r#"
        SELECT speech_count, contributions_text, summary, summary_model,
               summary_stale_at IS NULL
        FROM topic_speakers WHERE topic_id = $1 AND speaker_id = $2
        "#,
    )
    .bind(topic_id)
    .bind(speaker_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unchanged.0, 1);
    assert_eq!(unchanged.1, "Original contribution");
    assert_eq!(unchanged.2.as_deref(), Some("speaker summary"));
    assert_eq!(unchanged.3.as_deref(), Some("test-model"));
    assert!(unchanged.4);

    let contribution = sitting.sections[0].subsections[0].contributions[0].clone();
    sitting.sections[0].subsections[0].contributions[0].content = "Changed contribution".into();
    sitting.sections[0].subsections[0]
        .contributions
        .push(contribution);
    reconcile_extracted(&store, &sitting).await;

    let changed: (i32, String, Option<String>, Option<String>, bool, bool) = sqlx::query_as(
        r#"
        SELECT speech_count, contributions_text, summary, summary_model,
               summary_stale_at IS NOT NULL,
               summary_input_hash IS DISTINCT FROM input_hash
        FROM topic_speakers WHERE topic_id = $1 AND speaker_id = $2
        "#,
    )
    .bind(topic_id)
    .bind(speaker_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(changed.0, 2);
    assert_eq!(changed.1, "Changed contribution\n\nOriginal contribution");
    assert_eq!(changed.2.as_deref(), Some("speaker summary"));
    assert_eq!(changed.3.as_deref(), Some("test-model"));
    assert!(changed.4);
    assert!(changed.5);
    let topic_stale: (Option<String>, Option<String>, bool) = sqlx::query_as(
        "SELECT summary, summary_model, summary_stale_at IS NOT NULL FROM topics WHERE id = $1",
    )
    .bind(topic_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(topic_stale.0.as_deref(), Some("topic summary"));
    assert_eq!(topic_stale.1.as_deref(), Some("test-model"));
    assert!(topic_stale.2);

    sitting.sections.clear();
    reconcile_extracted(&store, &sitting).await;
    let inactive: (bool, bool, Option<String>) = sqlx::query_as(
        r#"
        SELECT t.active, ts.active, ts.summary
        FROM topics t JOIN topic_speakers ts ON ts.topic_id = t.id
        WHERE t.id = $1 AND ts.speaker_id = $2
        "#,
    )
    .bind(topic_id)
    .bind(speaker_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!inactive.0);
    assert!(!inactive.1);
    assert_eq!(inactive.2.as_deref(), Some("speaker summary"));

    sqlx::query("DELETE FROM sittings WHERE id = $1")
        .bind(sitting_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM speakers WHERE id = $1")
        .bind(speaker_id)
        .execute(&pool)
        .await
        .unwrap();
}

async fn reconcile_extracted(store: &PostgresStore, sitting: &odnelazm::HansardSitting) -> Uuid {
    reconcile_extracted_result(store, sitting).await.sitting_id
}

async fn reconcile_extracted_result(
    store: &PostgresStore,
    sitting: &odnelazm::HansardSitting,
) -> odnelazm_ingest::store::SittingReconciliation {
    let speakers = odnelazm_ingest::extract::extract_speakers(sitting);
    let bills = odnelazm_ingest::extract::extract_bills(sitting);
    let topics = odnelazm_ingest::extract::extract_topics(sitting);
    store
        .reconcile_sitting(sitting, "13th-parliament", &speakers, &bills, &topics)
        .await
        .unwrap()
}

fn synthetic_linkage_sitting(
    marker: Uuid,
    speakers: &[(String, Option<String>)],
) -> odnelazm::HansardSitting {
    let mut sitting = synthetic_topic_sitting(marker, "unused");
    sitting.source = odnelazm::DataSource::Current;
    sitting.url = format!("https://mzalendo.com/democracy-tools/hansard/integration-{marker}");
    sitting.sections[0].subsections[0].contributions = speakers
        .iter()
        .map(|(name, url)| odnelazm::Contribution {
            speaker_name: name.clone(),
            speaker_role: None,
            speaker_url: url.clone(),
            content: "Linkage contribution".into(),
            procedural_notes: vec![],
        })
        .collect();
    sitting
}

fn synthetic_topic_sitting(marker: Uuid, speaker_name: &str) -> odnelazm::HansardSitting {
    odnelazm::HansardSitting {
        house: odnelazm::House::NationalAssembly,
        date: chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap(),
        url: format!("https://info.mzalendo.com/hansard/sitting/integration/{marker}"),
        session_type: "Integration Test".into(),
        sections: vec![odnelazm::HansardSection {
            section_type: "STATEMENTS".into(),
            subsections: vec![odnelazm::HansardSubsection {
                title: format!("Integration Topic {marker}"),
                contributions: vec![odnelazm::Contribution {
                    speaker_name: speaker_name.into(),
                    speaker_role: None,
                    speaker_url: Some(format!("/person/integration-{marker}")),
                    content: "Original contribution".into(),
                    procedural_notes: vec![],
                }],
            }],
            contributions: vec![],
        }],
        source: odnelazm::DataSource::Archive,
        day_of_week: None,
        start_time: None,
        end_time: None,
        parliament_number: None,
        session_number: None,
        speaker_in_chair: None,
        summary: None,
        sentiment: None,
        pdf_url: None,
    }
}

async fn summary_counts(pool: &sqlx::PgPool) -> Vec<i64> {
    sqlx::query_scalar(
        r#"
        SELECT count(*) FROM (
            SELECT id FROM sittings WHERE generated_summary IS NOT NULL
            UNION ALL SELECT id FROM bills WHERE summary IS NOT NULL
            UNION ALL SELECT id FROM bill_mentions WHERE summary IS NOT NULL
            UNION ALL SELECT speaker_id FROM bill_mention_speakers WHERE summary IS NOT NULL
            UNION ALL SELECT id FROM topics WHERE summary IS NOT NULL
            UNION ALL SELECT speaker_id FROM topic_speakers WHERE summary IS NOT NULL
        ) summaries
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn stale_counts(pool: &sqlx::PgPool) -> Vec<i64> {
    sqlx::query_scalar(
        r#"
        SELECT count(*) FROM (
            SELECT id FROM sittings WHERE generated_summary_stale_at IS NOT NULL
            UNION ALL SELECT id FROM bills WHERE summary_stale_at IS NOT NULL
            UNION ALL SELECT id FROM bill_mentions WHERE summary_stale_at IS NOT NULL
            UNION ALL SELECT speaker_id FROM bill_mention_speakers WHERE summary_stale_at IS NOT NULL
            UNION ALL SELECT id FROM topics WHERE summary_stale_at IS NOT NULL
            UNION ALL SELECT speaker_id FROM topic_speakers WHERE summary_stale_at IS NOT NULL
        ) stale_summaries
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn derived_snapshot(pool: &sqlx::PgPool, sitting_id: Uuid) -> serde_json::Value {
    sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object(
            'sitting_speakers', (SELECT jsonb_agg(to_jsonb(x) ORDER BY speaker_id) FROM (
                SELECT speaker_id, speech_count, active, input_hash
                FROM sitting_speakers WHERE sitting_id = $1
            ) x),
            'bill_mentions', (SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM (
                SELECT bm.id, bm.speech_count, bm.active, bm.input_hash,
                       bm.summary, bm.summary_model, bm.summary_stale_at
                FROM bill_mentions bm WHERE bm.sitting_id = $1
            ) x),
            'bill_speakers', (SELECT jsonb_agg(to_jsonb(x) ORDER BY bill_mention_id, speaker_id) FROM (
                SELECT bms.bill_mention_id, bms.speaker_id, bms.speech_count,
                       bms.contributions_text, bms.active, bms.input_hash,
                       bms.summary, bms.summary_model, bms.summary_stale_at
                FROM bill_mention_speakers bms
                JOIN bill_mentions bm ON bm.id = bms.bill_mention_id
                WHERE bm.sitting_id = $1
            ) x),
            'topics', (SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM (
                SELECT t.id, t.speech_count, t.active, t.input_hash,
                       t.summary, t.summary_model, t.summary_stale_at
                FROM topics t WHERE t.sitting_id = $1
            ) x),
            'topic_speakers', (SELECT jsonb_agg(to_jsonb(x) ORDER BY topic_id, speaker_id) FROM (
                SELECT ts.topic_id, ts.speaker_id, ts.speech_count,
                       ts.contributions_text, ts.active, ts.input_hash,
                       ts.summary, ts.summary_model, ts.summary_stale_at
                FROM topic_speakers ts
                JOIN topics t ON t.id = ts.topic_id
                WHERE t.sitting_id = $1
            ) x)
        )
        "#,
    )
    .bind(sitting_id)
    .fetch_one(pool)
    .await
    .unwrap()
}
