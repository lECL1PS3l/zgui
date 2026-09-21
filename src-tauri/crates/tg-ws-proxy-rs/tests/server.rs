use std::time::Duration;

use clap::Parser;
use tg_ws_proxy_rs::config::Config;
use tg_ws_proxy_rs::server;

fn test_config(port: u16) -> Config {
    Config::try_parse_from([
        "tg-ws-proxy",
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--link-ip",
        "127.0.0.1",
        "--quiet",
        "--pool-size",
        "0",
    ])
    .unwrap()
    .with_defaults()
}

#[tokio::test]
async fn run_binds_then_stops_on_shutdown() {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (listen_tx, listen_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        server::run_with_listen(
            // Port 0 instead of a port a throwaway listener just released:
            // on a loaded CI runner something else can grab it in between.
            test_config(0),
            async {
                let _ = shutdown_rx.await;
            },
            move |info| {
                let _ = listen_tx.send(info);
            },
        )
        .await
    });

    let info = tokio::time::timeout(Duration::from_secs(5), listen_rx)
        .await
        .expect("server did not bind in time")
        .expect("listen callback dropped");

    assert_ne!(info.addr.port(), 0);
    assert!(
        info.tg_link
            .starts_with("tg://proxy?server=127.0.0.1&port="),
        "unexpected link: {}",
        info.tg_link
    );
    assert!(info.tg_link.contains(&format!("port={}", info.addr.port())));

    tokio::net::TcpStream::connect(info.addr)
        .await
        .expect("listener should accept connections");

    shutdown_tx.send(()).unwrap();
    server
        .await
        .expect("server task panicked")
        .expect("server returned an error");

    tokio::net::TcpListener::bind(info.addr)
        .await
        .expect("port should be released after stop");
}

#[tokio::test]
async fn port_zero_reports_the_real_bound_port_in_the_link() {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (listen_tx, listen_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        server::run_with_listen(
            test_config(0),
            async {
                let _ = shutdown_rx.await;
            },
            move |info| {
                let _ = listen_tx.send(info);
            },
        )
        .await
    });

    let info = tokio::time::timeout(Duration::from_secs(5), listen_rx)
        .await
        .expect("server did not bind in time")
        .expect("listen callback dropped");

    assert_ne!(info.addr.port(), 0);
    assert!(
        info.tg_link.contains(&format!("port={}", info.addr.port())),
        "link should use the bound port, got {}",
        info.tg_link
    );

    shutdown_tx.send(()).unwrap();
    server
        .await
        .expect("server task panicked")
        .expect("server returned an error");
}

#[tokio::test]
async fn default_domain_refresh_does_not_delay_listener_startup() {
    let mut config = test_config(0);
    config.default_domains = true;
    // If startup still awaited GitHub, this deliberately unreachable proxy
    // would hold the listener behind the outbound connect timeout.
    config.outbound_proxy = Some("http://192.0.2.1:9".to_string());
    config.no_proxy = Some(String::new());

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (listen_tx, listen_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        server::run_with_listen(
            config,
            async {
                let _ = shutdown_rx.await;
            },
            move |info| {
                let _ = listen_tx.send(info);
            },
        )
        .await
    });

    tokio::time::timeout(Duration::from_secs(1), listen_rx)
        .await
        .expect("live CF refresh delayed the local listener")
        .expect("listen callback dropped");

    shutdown_tx.send(()).unwrap();
    task.await.unwrap().unwrap();
}
