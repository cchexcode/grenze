use anyhow::Result;
use axum::{routing::{get, post}, Router};

pub mod api;

#[tokio::main]
async fn main() -> Result<()> {
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let state = loop {
        match api::proxy::AppState::new(1, &redis_url).await {
            Ok(s) => break s,
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
    };
    let app = Router::new()
        .route("/health", get(api::health::health))
        .route("/proxy", post(api::proxy::proxy))
        .with_state(state);

    println!("Starting server on 0.0.0.0:8080");
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", 8080)).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(signals())
        .await?;
    println!("Server has shut down gracefully");
    Ok(())
}

async fn signals() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = sigint.recv() => {
            println!("Received SIGINT. Shutting down...");
        }
        _ = sigterm.recv() => {
            println!("Received SIGTERM. Shutting down...");
        }
    }
}
