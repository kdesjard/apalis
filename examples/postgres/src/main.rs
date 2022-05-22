use apalis::{
    layers::tracing::TraceLayer, postgres::PostgresStorage, Job, JobContext, JobError, JobResult,
    Monitor, Storage, WorkerBuilder,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Email {
    to: String,
    subject: String,
    text: String,
}

impl Job for Email {
    const NAME: &'static str = "postgres::Email";
}

async fn email_service(_email: Email, _ctx: JobContext) -> Result<JobResult, JobError> {
    Ok(JobResult::Success)
}

async fn produce_jobs(storage: &PostgresStorage<Email>) {
    let mut storage = storage.clone();
    for i in 0..1000 {
        storage
            .push(Email {
                to: format!("test{}@example.com", i),
                text: "Test backround job from Apalis".to_string(),
                subject: "Background email job".to_string(),
            })
            .await
            .unwrap();
    }
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "debug,sqlx::query=error");
    tracing_subscriber::fmt::init();
    let database_url = std::env::var("DATABASE_URL").expect("Must specify path to db");

    let pg: PostgresStorage<Email> = PostgresStorage::connect(database_url).await.unwrap();
    pg.setup()
        .await
        .expect("unable to run migrations for postgres");

    produce_jobs(&pg).await;

    Monitor::new()
        .register_with_count(4, move |_| {
            WorkerBuilder::new(pg.clone())
                .layer(TraceLayer::new())
                .build_fn(email_service)
                .start()
        })
        .run()
        .await
}
