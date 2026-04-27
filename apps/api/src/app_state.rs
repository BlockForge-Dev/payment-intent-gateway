use application::{PaymentIntentService, ProviderWebhookService};
use persistence::PostgresPersistence;

#[derive(Clone)]
pub struct AppState {
    pub service: PaymentIntentService<PostgresPersistence>,
    pub webhook_service: ProviderWebhookService<PostgresPersistence>,
    pub api_bearer_token: String,
    pub mock_provider_webhook_secret: Option<String>,
}
