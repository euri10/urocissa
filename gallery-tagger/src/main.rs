use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use gallery_tagger::{api, config::Config, embedder::Embedder, openvino_backend::OpenVinoEmbedder};
use log::info;

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck();
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match serve() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

#[tokio::main]
async fn serve() -> Result<()> {
    let config = Config::from_env()?;
    let model_dir = config.model_dir.clone();
    let backend = config.backend.clone();
    let embedder: Arc<dyn Embedder> = Arc::new(
        tokio::task::spawn_blocking(move || OpenVinoEmbedder::load(&model_dir, &backend))
            .await
            .context("model loading task failed")??,
    );
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))?;
    info!("listening on {}", config.listen);
    axum::serve(listener, api::router(embedder, config.limits))
        .with_graceful_shutdown(shutdown())
        .await
        .context("serving HTTP")
}

async fn shutdown() {
    let interrupt = tokio::signal::ctrl_c();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing SIGTERM handler");
    tokio::select! {
        _ = interrupt => {},
        _ = terminate.recv() => {},
    }
    info!("shutting down");
}

/// `gallery-tagger healthcheck`: exit 0 when the local `/healthz` answers 200.
/// Lets container health checks work without curl in the image.
fn healthcheck() -> ExitCode {
    let check = || -> Result<bool> {
        let port = Config::from_env()?.listen.port();
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut status = [0_u8; 12];
        stream.read_exact(&mut status)?;
        Ok(status.starts_with(b"HTTP/1.1 200"))
    };
    match check() {
        Ok(true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
