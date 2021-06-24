use std::{net::SocketAddr, process::Stdio};

use tokio::process::Command;
use warp::{Filter, Rejection};

static GIT_PROJECT_ROOT: &str = "/root/test/";

#[tokio::main]
async fn main() {
    // GET /hello/warp => 200 OK with body "Hello, warp!"
    let hello = 
        warp::path!("hello" / String)
            .map(|name| format!("Hello, {}!", name));

    let git = 
        warp::path("git")
            .and(warp::path("crates.io-index-test"))
            .and(warp::method())
            .and(warp::header::optional::<String>("Content-Type"))
            .and(warp::header::optional::<String>("Content-Encoding"))
            .and(warp::query::raw())
            .and(warp::addr::remote())
            .and_then(handle_git);

    warp::serve(hello.or(git))
        .run(([0, 0, 0, 0], 3030))
        .await;
}

async fn handle_git(method: http::Method, content_type: Option<String>, encoding: Option<String>, query: String, remote: Option<SocketAddr>) -> Result<String, Rejection> {
    dbg!(&method, &content_type, &encoding, &query, &remote);

    let remote = remote.map(|r| r.ip().to_string()).unwrap_or_else(|| "127.0.0.1".to_string());

    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.env_clear();
    cmd.env("GIT_PROJECT_ROOT", GIT_PROJECT_ROOT);
    cmd.env("PATH_INFO", "/crates.io-index");
    cmd.env("REQUEST_METHOD", method.as_str());
    cmd.env("QUERY_STRING", query);
    cmd.env("REMOTE_USER", "");
    cmd.env("REMOTE_ADDR", remote);
    cmd.stderr(Stdio::inherit());
    cmd.stdout(Stdio::piped());
    cmd.stdin(Stdio::piped());

    let p = cmd.spawn().unwrap();

    dbg!(&p);
    
    Ok(format!("Hello!"))
}