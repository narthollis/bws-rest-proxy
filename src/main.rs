mod bw;

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use bw::{get_secret, ErrorMessage};
use clap::Parser;
use hyper::StatusCode;
use std::{
    env,
    net::{IpAddr, SocketAddr},
    str::FromStr,
};
use tokio::signal::{self, unix::SignalKind};
use tracing::*;

use crate::bw::settings;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(default_value = "0.0.0.0")]
    listen_address: String,
    #[arg(default_value = "3030")]
    listen_port: u16,
    #[arg(long)]
    health_port: Option<u16>,
    #[arg(long, requires = "health_port")]
    health_address: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let ip = IpAddr::from_str(cli.listen_address.as_str())?;
    let port = cli.listen_port;
    let health_ip = cli
        .health_address
        .and_then(|a| IpAddr::from_str(a.as_str()).ok());

    tracing_subscriber::fmt()
        .without_time()
        .with_level(true)
        .with_target(true)
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .with_max_level(tracing::Level::INFO)
        .init();

    let settings = settings(
        env::var("BWS_IDENTITY_URL").ok(),
        env::var("BWS_API_URL").ok(),
    );

    let app = Router::new()
        .route("/:org_id/:project_id/secret/:secret_id", get(get_secret))
        .route("/_health", get(health))
        .with_state(settings)
        .fallback(not_found_handler);

    let addr = SocketAddr::from((ip, port));

    let health_server;

    if let Some(a) = health_ip {
        if let Some(p) = cli.health_port {
            let health_addr = SocketAddr::from((a, p));
            if health_addr != addr {
                let real_addr = match addr.ip().is_unspecified() {
                    true => "127.0.0.1".to_string(),
                    false => addr.ip().to_string(),
                };
                let client = reqwest::Client::new();
                let r = Router::new().route("/_health", get(health_fw)).with_state((
                    client,
                    real_addr,
                    addr.port(),
                ));
                health_server = Some(
                    hyper::Server::bind(&health_addr)
                        .serve(r.into_make_service())
                        .with_graceful_shutdown(shutdown_signal()),
                );
            } else {
                health_server = None;
            }
        } else {
            health_server = None;
        }
    } else {
        health_server = None
    }

    let server = hyper::Server::bind(&addr)
        .serve(app.into_make_service())
        .with_graceful_shutdown(shutdown_signal());

    info!(address = format!("{addr:?}"), "Listening, ctrl+c to exit");
    futures::future::join_all(match health_server {
        Some(s) => vec![server, s],
        None => vec![server],
    })
    .await
    .into_iter()
    .collect::<Result<_, hyper::Error>>()?;

    Ok(())
}

async fn health() -> &'static str {
    "Ok"
}

async fn health_fw(
    State((client, addr, port)): State<(reqwest::Client, String, u16)>,
) -> axum::response::Response {
    let url = format!("http://{addr}:{port}/_health");
    match client.get(&url).send().await {
        Ok(r) => {
            let status = r.status();
            match r.text().await {
                Ok(b) => (status, b),
                Err(e) => {
                    info!(
                        error = format!("{e:?}"),
                        url = url,
                        "Failed to read body of forwarded health check"
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal Server Error".into(),
                    )
                }
            }
        }
        Err(e) => {
            info!(
                error = format!("{e:?}"),
                url = url,
                "Failed to forward health check"
            );
            (
                e.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                "Internal Server Error".into(),
            )
        }
    }
    .into_response()
}

async fn not_found_handler() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorMessage {
            code: StatusCode::NOT_FOUND,
            message: "Not Found".into(),
        }),
    )
        .into_response()
}

async fn shutdown_signal() {
    let mut sigint = signal::unix::signal(SignalKind::interrupt()).expect("Failed to bind SIGINT");
    let mut sigterm =
        signal::unix::signal(SignalKind::terminate()).expect("Failed to bind SIGTERM");

    let kind = tokio::select! {
        _ = signal::ctrl_c() => "Ctrl+C",
        _ = sigint.recv() => "SIGINT",
        _ = sigterm.recv() => "SIGTERM",
    };

    info!("Got {kind} - Shutting Down")
}

impl IntoResponse for ErrorMessage {
    fn into_response(self) -> axum::response::Response {
        let body = Json(&self);

        (self.code, body).into_response()
    }
}
