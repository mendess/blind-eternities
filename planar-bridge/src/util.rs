use axum::{Router, response::Redirect, routing::get};

pub fn append_slash_router<S>(routes: &[&'static str]) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let mut router = Router::new();
    for r in routes {
        router = router.route(r, get(Redirect::to(&format!(".{r}/"))));
    }
    router
}
