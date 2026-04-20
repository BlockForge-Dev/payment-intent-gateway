use application::PaymentIntentService;
use persistence::PostgresPersistence;

#[derive(Clone)]
pub struct AppState {
    pub service: PaymentIntentService<PostgresPersistence>,
    pub api_bearer_token: String,
}
