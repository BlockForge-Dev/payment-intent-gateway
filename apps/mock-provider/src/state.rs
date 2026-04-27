use std::{collections::HashMap, sync::Arc};

use reqwest::Client;
use tokio::sync::RwLock;

use crate::models::SimulatedPayment;

pub type PaymentStore = Arc<RwLock<HashMap<String, SimulatedPayment>>>;

#[derive(Clone)]
pub struct AppState {
    pub store: PaymentStore,
    pub http_client: Client,
    pub webhook_secret: Option<String>,
}

impl AppState {
    pub fn new(webhook_secret: Option<String>) -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
            http_client: Client::new(),
            webhook_secret,
        }
    }
}
