async fn async_main() {
    let app = aperture_http::router().await;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    eprintln!("Listening on http://0.0.0.0:8000");
    axum::serve(listener, app).await.unwrap()
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async_main())
}
