use crate::git_store::store_entry::Store;
use crate::nix_interface::cache_info;
use actix_web::{
    App, HttpResponse, HttpServer, Responder, get, head,
    web::{Data, Path},
};
use tracing_actix_web::TracingLogger;

#[get("/nix-cache-info")]
async fn nix_cache_info() -> impl Responder {
    let default_cache_info = cache_info::CacheInfo::default();
    HttpResponse::Ok().body(default_cache_info.to_string())
}

#[get("/{nix_hash}.narinfo")]
async fn get_narinfo(cache: Data<Store>, path: Path<String>) -> impl Responder {
    let cache = cache.into_inner();
    let hash = path.into_inner();
    let res = cache.get_narinfo(&hash);
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
async fn get_nar(cache: Data<Store>, path: Path<String>) -> impl Responder {
    let cache = cache.into_inner();
    let hash = path.into_inner();

    match cache.get_as_nar_stream(&hash) {
        Ok(Some(nar_stream)) => HttpResponse::Ok().streaming(nar_stream),
        Ok(None) => HttpResponse::NotFound().body("Entry is not in the Cache"),
        _ => HttpResponse::InternalServerError().body("Server error while fetching entry"),
    }
}

#[head("/{nix_hash}.narinfo")]
async fn nar_exists(cache: Data<Store>, path: Path<String>) -> impl Responder {
    let cache = cache.into_inner();
    let hash = path.into_inner();

    match cache.entry_exists(&hash) {
        Ok(true) => HttpResponse::Ok(),
        _ => HttpResponse::NotFound(),
    }
}

#[actix_web::main]
pub async fn start_server(host: &str, port: u16, store: Store) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .app_data(Data::new(store.clone()))
            .service(get_narinfo)
            .service(nix_cache_info)
            .service(nar_exists)
            .service(get_nar)
            .service(get_listing)
    })
    .bind((host, port))?
    .run()
    .await
}
