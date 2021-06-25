use std::{collections::HashMap, net::SocketAddr, process::Stdio};

use http::Response;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use warp::{Filter, Rejection, path::Tail};

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
        .and_then(handle_git);

    warp::serve(hello.or(git)).run(([0, 0, 0, 0], 3030)).await;
}

async fn handle_git(
    path_tail: Tail,
    method: http::Method,
    content_type: Option<String>,
    encoding: Option<String>,
    query: String,
    remote: Option<SocketAddr>,
) -> Result<String, Rejection> {
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
    cmd.env("GIT_HTTP_EXPORT_ALL", "true");
    cmd.stderr(Stdio::inherit());
    cmd.stdout(Stdio::piped());
    cmd.stdin(Stdio::piped());

    let p = cmd.spawn().unwrap();

    let out = BufReader::new(p.stdout.unwrap());

    let mut headers = HashMap::new();

    {
        let mut out_lines = out.lines();
        while let Some(line) = out_lines.next_line().await.unwrap() {
            println!("line: {:?}", line);
            if line.is_empty() {
                break;
            }
            if let Some((key, value)) = line.split_once(": ") {
                headers.insert(key.to_string(), value.to_string());
            }
        }
    }

    dbg!(&headers);

    let mut resp = Response::builder();
    for (key, val) in headers {
        if key == "Status" {
            resp = resp.status(&val.as_bytes()[..3]);
        } else {
            resp = resp.header(&key, val);
        }
    }

    dbg!(resp);


    Ok(format!("Hello!"))
}
