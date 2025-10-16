use crate::git_store::{cache_info, nar_info, store_entry};
use actix_web::{
    App, HttpResponse, HttpServer, Responder, get, head,
    web::{Data, Path},
};
use std::sync::Arc;
use tracing_actix_web::TracingLogger;

use crate::git_store::GitStore;

#[get("/nix-cache-info")]
async fn nix_cache_info() -> impl Responder {
    let default_cache_info = cache_info::CacheInfo::default();
    HttpResponse::Ok().body(default_cache_info.to_string())
}

#[get("/{nix_hash}.narinfo")]
async fn get_narinfo(cache: Data<Arc<GitStore>>, path: Path<String>) -> impl Responder {
    let cache = cache.into_inner();
    let hash = path.into_inner();
    let res = nar_info::get_from_tree(&cache, &hash);
    match res {
        Ok(Some(nar_info)) => HttpResponse::Ok().body(nar_info),
        Ok(None) => HttpResponse::NotFound().body("Entry is not in the Cache"),
        _ => HttpResponse::InternalServerError().body("Server error while fetching narinfo entry"),
    }
}

#[get("/nar/{nix_hash}.ls")]
async fn get_listing(path: Path<String>) -> impl Responder {
    let hash = path.into_inner();
    HttpResponse::Ok().body(hash)
}

#[get("/nar/{file_hash}.nar")]
async fn get_compressed_nar(cache: Data<Arc<GitStore>>, path: Path<(String)>) -> impl Responder {
    let cache = cache.into_inner();
    let hash = path.into_inner();
    match store_entry::get_as_nar(&cache, &hash) {
        Ok(Some(nar)) => HttpResponse::Ok().body(nar),
        Ok(None) => HttpResponse::NotFound().body("Entry is not in the Cache"),
        _ => HttpResponse::InternalServerError().body("Server error while fetching entry"),
    }
}

#[head("/{nix_hash}.narinfo")]
async fn nar_exists(cache: Data<Arc<GitStore>>, path: Path<String>) -> impl Responder {
    let cache = cache.into_inner();
    let hash = path.into_inner();

    if nar_info::exists(&cache, &hash) {
        HttpResponse::Ok()
    } else {
        HttpResponse::NotFound()
    }
}

#[actix_web::main]
pub async fn start_server(host: &str, port: u16, cache: GitStore) -> std::io::Result<()> {
    let cache = Arc::new(cache);
    HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .app_data(Data::new(cache.clone()))
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
