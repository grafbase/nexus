use indoc::indoc;
use integration_tests::*;

#[tokio::test]
async fn default_path() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/mcp").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @"<h1>Hello, World!</h1>");
}

#[tokio::test]
async fn custom_path() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
        path = "/custom"
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/custom").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @"<h1>Hello, World!</h1>");
}

#[tokio::test]
async fn successful_tls_connection() {
    let config = indoc! {r#"
        [server]
        [server.tls]
        certificate = "certs/cert.pem"
        key = "certs/key.pem"

        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/mcp").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @"<h1>Hello, World!</h1>");
}
