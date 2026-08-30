use odnelazm_ingest::store::{IngestionRunCompletion, IngestionRunStatus};
use odnelazm_ingest::{DataStore, PostgresStore};
use sqlx::postgres::PgPoolOptions;

const LOCAL_TEST_DATABASE_URL: &str = "postgres://odnelazm:odnelazm@localhost:5432/odnelazm";

#[tokio::test]
#[ignore = "requires the explicitly configured local integration database"]
async fn ingestion_run_transitions_to_partial_with_consistent_counts() {
    let database_url = std::env::var("ODNELAZM_TEST_DATABASE_URL")
        .expect("ODNELAZM_TEST_DATABASE_URL must be explicitly set");
    assert_eq!(database_url, LOCAL_TEST_DATABASE_URL);

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .unwrap();
    let store = PostgresStore::from_pool(pool.clone());
    store.migrate().await.unwrap();

    let run_id = store
        .start_ingestion_run(
            "mzalendo-current",
            serde_json::json!({ "operation": "integration_test" }),
        )
        .await
        .unwrap();
    let running: (String, bool, bool) = sqlx::query_as(
        "SELECT status, started_at IS NOT NULL, finished_at IS NULL FROM ingestion_runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(running, ("running".to_owned(), true, true));

    store
        .finish_ingestion_run(
            run_id,
            &IngestionRunCompletion {
                status: IngestionRunStatus::Partial,
                discovered: 3,
                processed: 3,
                succeeded: 1,
                skipped: 1,
                failed: 1,
                error_message: Some("partial result".to_owned()),
                error_metadata: serde_json::json!({ "reason": "test" }),
            },
        )
        .await
        .unwrap();

    let completed: (String, i64, i64, i64, i64, i64, bool) = sqlx::query_as(
        r#"
        SELECT status, discovered_count, processed_count, succeeded_count,
               skipped_count, failed_count, finished_at IS NOT NULL
        FROM ingestion_runs WHERE id = $1
        "#,
    )
    .bind(run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(completed, ("partial".to_owned(), 3, 3, 1, 1, 1, true));
    assert!(
        store
            .finish_ingestion_run(
                run_id,
                &IngestionRunCompletion {
                    status: IngestionRunStatus::Succeeded,
                    discovered: 0,
                    processed: 0,
                    succeeded: 0,
                    skipped: 0,
                    failed: 0,
                    error_message: None,
                    error_metadata: serde_json::json!({}),
                },
            )
            .await
            .is_err()
    );

    sqlx::query("DELETE FROM ingestion_runs WHERE id = $1")
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap();
}
