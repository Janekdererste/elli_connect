use actix_web::{HttpResponse, Responder};
use askama::Template;

// Template definitions
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub title: String,
    pub greeting: String,
}

pub fn into_response<T: Template>(template: T) -> impl Responder {

    match template.render() {
        Ok(rendered) => { HttpResponse::Ok().body(rendered)}
        Err(_) => {HttpResponse::InternalServerError().body("<h1>Error</h1><p>Uh oh, an error while rendering a template</p>")}
    }
}
