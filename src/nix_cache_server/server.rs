use actix_web::{App, HttpResponse, HttpServer, Responder, get, head, web::Path};
use tracing_actix_web::TracingLogger;

// use crate::git_store::GitStore;

#[get("/nix-cache-info")]
async fn nix_cache_info() -> impl Responder {
    HttpResponse::Ok().body("hi")
}

#[head("/{nix_hash}.narinfo")]
async fn get_narinfo(path: Path<String>) -> impl Responder {
    let hash = path.into_inner();
    if !hash.is_empty() {
        HttpResponse::Ok()
    } else {
        HttpResponse::NotFound()
    }
}

#[get("/nar/{nix_hash}.ls")]
async fn get_listing(path: Path<String>) -> impl Responder {
    let hash = path.into_inner();
    HttpResponse::Ok().body(hash)
}

#[get("/nar/{file_hash}.nar.{compression}")]
async fn get_compressed_nar(path: Path<(String, String)>) -> impl Responder {
    let (hash, compression) = path.into_inner();
    HttpResponse::Ok().body(format!(
        "Hash: {}\nCompression Method: {}",
        hash, compression
    ))
}

#[get("/{nix_hash}.narinfo")]
async fn nar_exists(path: Path<String>) -> impl Responder {
    let hash = path.into_inner();
    HttpResponse::Ok().body(hash)
}

#[actix_web::main]
pub async fn start_server(host: &str, port: u16) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .service(get_narinfo)
            .service(nix_cache_info)
            .service(nar_exists)
            .service(get_compressed_nar)
            .service(get_listing)
    })
    .bind((host, port))?
    .run()
    .await
}
