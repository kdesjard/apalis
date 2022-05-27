//! Run with
//!
//! ```not_rust
//! cd examples && cargo run -p prometheus-example
//! ```

use apalis::{
    layers::prometheus::PrometheusLayer, redis::RedisStorage, Job, JobContext, JobError, JobResult,
    Monitor, Storage, WorkerBuilder,
};
use axum::{
    extract::Form,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Extension, Router,
};
use futures::future::ready;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt::Debug, net::SocketAddr};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Deserialize, Serialize)]

struct Email {
    to: String,
    subject: String,
    text: String,
}

impl Job for Email {
    const NAME: &'static str = "redis::Email";
}

async fn email_service(job: Email, ctx: JobContext) -> Result<JobResult, JobError> {
    // Do something awesome
    println!("Attempting to send email to {}", job.to);
    Ok(JobResult::Success)
}

fn main() {
    actix_rt::System::with_tokio_rt(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    })
    .block_on(async {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(
                std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
            ))
            .with(tracing_subscriber::fmt::layer())
            .init();
        let storage: RedisStorage<Email> =
            RedisStorage::connect("redis://127.0.0.1/").await.unwrap();
        // build our application with some routes
        let recorder_handle = setup_metrics_recorder();
        let app = Router::new()
            .route("/", get(show_form).post(add_new_job::<Email>))
            .layer(Extension(storage.clone()))
            .route("/metrics", get(move || ready(recorder_handle.render())));

        // run it with hyper
        let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
        tracing::debug!("listening on {}", addr);
        let http = async {
            axum::Server::bind(&addr)
                .serve(app.into_make_service())
                .await
                .map_err(|_e| ())
        };
        let monitor = async {
            Monitor::new()
                .register_with_count(2, move |_| {
                    WorkerBuilder::new(storage.clone())
                        .layer(PrometheusLayer)
                        .build_fn(email_service)
                        .start()
                })
                .run()
                .await
                .map_err(|_e| ())
        };
        futures::future::try_join(monitor, http).await.unwrap();
    })
}

fn setup_metrics_recorder() -> PrometheusHandle {
    const EXPONENTIAL_SECONDS: &[f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];

    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("job_requests_duration_seconds".to_string()),
            EXPONENTIAL_SECONDS,
        )
        .unwrap()
        .install_recorder()
        .unwrap()
}

async fn show_form() -> Html<&'static str> {
    Html(
        r#"
        <!doctype html>
        <html>
            <head>
                <link href="https://unpkg.com/tailwindcss@1.2.0/dist/tailwind.min.css" rel="stylesheet">
                <meta credits="https://tailwindcomponents.com/component/basic-contact-form" />
            </head>
            <body>
                <form style="margin: 0 auto;" class="w-full max-w-lg pt-20" action="/" method="post">
                    <div class="flex flex-wrap -mx-3 mb-6">
                    <div class="w-full md:w-2/3 px-3 mb-6 md:mb-0">
                        <label class="block uppercase tracking-wide text-gray-700 text-xs font-bold mb-2" for="to">
                        To
                        </label>
                        <input class="appearance-none block w-full bg-gray-200 text-gray-700 border border-red-500 rounded py-3 px-4 mb-3 leading-tight focus:outline-none focus:bg-white" id="to" type="email" name="to" placeholder="test@example.com">
                        <p class="text-red-500 text-xs italic">Please fill out this field.</p>
                    </div>

                    </div>
                    <div class="flex flex-wrap -mx-3 mb-6">
                    <div class="w-full px-3">
                        <label class="block uppercase tracking-wide text-gray-700 text-xs font-bold mb-2" for="subject">
                        Subject
                        </label>
                        <input class="appearance-none block w-full bg-gray-200 text-gray-700 border border-gray-200 rounded py-3 px-4 mb-3 leading-tight focus:outline-none focus:bg-white focus:border-gray-500" id="subject" type="text" name="subject">
                        <p class="text-gray-600 text-xs italic">Some tips - as long as needed</p>
                    </div>
                    </div>
                    <div class="flex flex-wrap -mx-3 mb-6">
                    <div class="w-full px-3">
                        <label class="block uppercase tracking-wide text-gray-700 text-xs font-bold mb-2" for="text">
                        Message
                        </label>
                        <textarea class=" no-resize appearance-none block w-full bg-gray-200 text-gray-700 border border-gray-200 rounded py-3 px-4 mb-3 leading-tight focus:outline-none focus:bg-white focus:border-gray-500 h-48 resize-none" id="text" name="text" ></textarea>
                    </div>
                    </div>
                    <div class="md:flex md:items-center">
                    <div class="md:w-1/3">
                        <button class="shadow bg-teal-400 hover:bg-teal-400 focus:shadow-outline focus:outline-none text-white font-bold py-2 px-4 rounded" type="submit">
                        Send
                        </button>
                    </div>
                    <div class="md:w-2/3"></div>
                    </div>
                </form>
            </body>
        </html>
        "#,
    )
}

async fn add_new_job<T>(
    Form(input): Form<T>,
    Extension(mut storage): Extension<RedisStorage<T>>,
) -> impl IntoResponse
where
    T: 'static + Debug + Job + Serialize + DeserializeOwned + Unpin + Send,
{
    dbg!(&input);
    let new_job = storage.push(input).await;

    match new_job {
        Ok(()) => (
            StatusCode::CREATED,
            "Job was successfully added".to_string(),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("An Error occured {}", e),
        ),
    }
    .into_response()
}