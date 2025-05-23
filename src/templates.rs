use actix_web::HttpResponse;
use askama::Template;

// Template definitions
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {}

#[derive(Template)]
#[template(path = "callback.html")]
pub struct CallbackTemplate {
    pub scope: String,
}

pub fn into_response<T: Template>(template: T) -> HttpResponse {
    match template.render() {
        Ok(rendered) => HttpResponse::Ok().body(rendered),
        Err(_) => HttpResponse::InternalServerError()
            .body("<h1>Error</h1><p>Uh oh, an error while rendering a template</p>"),
    }
}
