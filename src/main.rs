mod templates;

use crate::templates::into_response;
use actix_files as fs;
use actix_web::{get, App, HttpServer, Responder};
use templates::IndexTemplate;

#[get("/")]
async fn index() -> impl Responder {
    let template = IndexTemplate {
        title: "Welcome".to_string(),
        greeting: "Hello, welcome to our web application!".to_string(),
    };

    into_response(template)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Server starting at http://127.0.0.1:8080");
    
    HttpServer::new(|| {
        App::new()
            .service(index)
            .service(fs::Files::new("/static", "./static").show_files_listing())
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}