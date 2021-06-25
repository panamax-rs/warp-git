use std::{collections::HashMap, net::SocketAddr, process::Stdio};

use http::Response;
use hyper::{Body, body::Sender};
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader}, process::{ChildStdout, Command}};
use tokio_stream::StreamExt;
use warp::{Filter, Rejection, path::Tail};
use futures::Stream;
use bytes::BytesMut;

static GIT_PROJECT_ROOT: &str = "/root/test";

#[tokio::main]
async fn main() {
    // GET /hello/warp => 200 OK with body "Hello, warp!"
    let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));

    let git = warp::path("git")
        .and(warp::path("crates.io-index"))
        .and(warp::path::tail())
        .and(warp::method())
        .and(warp::header::optional::<String>("Content-Type"))
        .and(warp::header::optional::<String>("Content-Encoding"))
        .and(warp::query::raw())
        .and(warp::addr::remote())
        .and(warp::body::stream())
        .and_then(handle_git);

    // query::raw() seems to expect a non-empty query string
    // so create a separate set of filters for when it's empty
    let git_no_query = warp::path("git")
        .and(warp::path("crates.io-index"))
        .and(warp::path::tail())
        .and(warp::method())
        .and(warp::header::optional::<String>("Content-Type"))
        .and(warp::header::optional::<String>("Content-Encoding"))
        .and(warp::addr::remote())
        .and(warp::body::stream())
        .and_then(handle_git_empty_query);

    warp::serve(hello.or(git).or(git_no_query)).run(([0, 0, 0, 0], 3030)).await;
}

/// Handle a request from a git client.
/// Special case for empty query strings.
async fn handle_git_empty_query<S, B>(
    path_tail: Tail,
    method: http::Method,
    content_type: Option<String>,
    encoding: Option<String>,
    remote: Option<SocketAddr>,
    body: S,
) -> Result<Response<Body>, Rejection> 
    where
        S: Stream<Item = Result<B, warp::Error>> + Send + Unpin + 'static,
        B: bytes::Buf + Sized
{
    handle_git(path_tail, method, content_type, encoding, String::new(), remote, body).await
}

/// Handle a request from a git client.
async fn handle_git<S, B>(
    path_tail: Tail,
    method: http::Method,
    content_type: Option<String>,
    encoding: Option<String>,
    query: String,
    remote: Option<SocketAddr>,
    mut body: S,
) -> Result<Response<Body>, Rejection> 
    where
        S: Stream<Item = Result<B, warp::Error>> + Send + Unpin + 'static,
        B: bytes::Buf + Sized
{
    dbg!(
        &path_tail,
        &method,
        &content_type,
        &encoding,
        &query,
        &remote
    );

    let remote = remote
        .map(|r| r.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.env_clear();
    cmd.env("GIT_PROJECT_ROOT", GIT_PROJECT_ROOT);
    cmd.env(
        "PATH_INFO",
        format!("/crates.io-index/{}", path_tail.as_str()),
    );
    cmd.env("REQUEST_METHOD", method.as_str());
    cmd.env("QUERY_STRING", query);
    cmd.env("REMOTE_USER", "");
    cmd.env("REMOTE_ADDR", remote);
    if let Some(content_type) = content_type {
        cmd.env("CONTENT_TYPE", content_type);
    }
    cmd.env("GIT_HTTP_EXPORT_ALL", "true");
    cmd.stderr(Stdio::inherit());
    cmd.stdout(Stdio::piped());
    cmd.stdin(Stdio::piped());

    let p = cmd.spawn().unwrap();

    let mut git_input = p.stdin.unwrap();
    // Handle sending body to http-backend, if any
    while let Some(Ok(mut buf)) = body.next().await {
        git_input.write_all_buf(&mut buf).await.unwrap();
    }

    let mut git_output = BufReader::new(p.stdout.unwrap());

    // Collect headers from git CGI output
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        git_output.read_line(&mut line).await.unwrap();

        let line = line.trim_end();
        if line.is_empty() {
            break;
        }

        if let Some((key, value)) = line.split_once(": ") {
            headers.insert(key.to_string(), value.to_string());
        }
    }
    dbg!(&headers);

    // Add headers to response (except for Status, which is the "200 OK" line)
    let mut resp = Response::builder();
    for (key, val) in headers {
        if key == "Status" {
            resp = resp.status(&val.as_bytes()[..3]);
        } else {
            resp = resp.header(&key, val);
        }
    }

    // Create channel, so data can be streamed without being fully loaded
    // into memory. Requires a separate future to be spawned.
    let (sender, body) = Body::channel();
    tokio::spawn(send_git(sender, git_output));

    let resp = resp.body(body).unwrap();
    Ok(resp)
}

/// Send data from git CGI process to hyper Sender, until there is no more
/// data left.
async fn send_git(mut sender: Sender, mut git_output: BufReader<ChildStdout>) {
    loop {
        let mut bytes_out = BytesMut::new();
        git_output.read_buf(&mut bytes_out).await.unwrap();
        if bytes_out.is_empty() {
            return;
        }
        sender.send_data(bytes_out.freeze()).await.unwrap();
    }
}