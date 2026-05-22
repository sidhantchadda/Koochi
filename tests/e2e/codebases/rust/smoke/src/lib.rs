pub struct Client;

impl Client {
    pub async fn get(&self) {}
}

pub async fn call_api(client: Client) {
    client.get().await;
}
