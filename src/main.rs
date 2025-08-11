use auto_batching_proxy::{Config, serve};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // We are only using the dotenvy crate's functionality when developing
    // the application. Having untracked `.env` is useful for storing API keys
    // that we do not want to expose, but _do_ want to use, for example, for
    // end-to-end testing using the workstation.
    //
    // To avoid maintenance overhead, we are keeping `.env` file as minimalistic
    // as possible, with almost all the entries being available for copy-pasting
    // from the `.env.example` (which we are also utilizing for documentation).
    #[cfg(debug_assertions)]
    {
        use dotenvy::dotenv;
        dotenv().ok();
    }

    // ------------------------- INITIALIZE TELEMETRY  -------------------------
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // --------------------- BUILD CONFIGURATION APPLICATION -------------------
    let config = match Config::try_build() {
        Err(e) => panic!("Failed to build applications's configuration: {:?}", e),
        Ok(config) => config,
    };

    // --------------------------- RUN APPLICATION -----------------------------
    if let Err(e) = serve(config).await {
        panic!("Failed to start application: {:?}", e);
    }
}
