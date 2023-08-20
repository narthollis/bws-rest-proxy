use bitwarden::{
    auth::request::AccessTokenLoginRequest,
    client::client_settings::{ClientSettings, DeviceType},
    secrets_manager::secrets::{SecretGetRequest, SecretResponse},
    Client,
};
use serde::ser::{Serialize, SerializeStruct};
use std::{
    convert::Infallible,
    env,
    net::{IpAddr, SocketAddr},
    str::FromStr,
};
use tokio::signal::{self, unix::SignalKind};
use tracing::*;
use uuid::Uuid;
use warp::{hyper::StatusCode, Filter};

#[derive(Debug, Clone)]
struct ErrorMessage {
    code: StatusCode,
    message: String,
}

impl warp::reject::Reject for ErrorMessage {}

impl Into<warp::reply::WithStatus<warp::reply::Json>> for ErrorMessage {
    fn into(self) -> warp::reply::WithStatus<warp::reply::Json> {
        warp::reply::with_status(warp::reply::json(&self), self.code)
    }
}

impl Serialize for ErrorMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("ErrorMessage", 3)?;

        state.serialize_field("code", &self.code.as_u16())?;
        state.serialize_field("message", &self.message)?;
        state.end()
    }
}

#[derive(serde::Deserialize)]
struct ResponseContent {
    message: String,
}

fn client_settings() -> ClientSettings {
    let identity_url =
        env::var("BWS_IDENTITY_URL").unwrap_or("https://identity.bitwarden.com".to_string());
    let api_url = env::var("BWS_API_URL").unwrap_or("https://api.bitwarden.com".to_string());

    ClientSettings {
        identity_url,
        api_url,
        user_agent: "bws_rest_proxy".to_string(),
        device_type: DeviceType::SDK,
    }
}

async fn get_secret(
    org_id: Uuid,
    project_id: Uuid,
    secret_id: Uuid,
    access_token: String,
) -> Result<SecretResponse, ErrorMessage> {
    info!(
        org_id = format!("{org_id:?}"),
        project_id = format!("{project_id:?}"),
        secret_id = format!("{secret_id:?}"),
        "get_secret request"
    );

    let mut client = Client::new(Some(client_settings()));

    let token_request = &AccessTokenLoginRequest { access_token };
    if let Err(e) = client.access_token_login(token_request).await {
        error!(login_error = format!("{e:?}"), "login error");
        return Err(ErrorMessage {
            code: StatusCode::UNAUTHORIZED,
            message: "Unauthorized".into(),
        });
    }

    let request = SecretGetRequest { id: secret_id };

    match client.secrets().get(&request).await {
        Ok(r) => {
            if r.organization_id != org_id {
                Err(ErrorMessage {
                    code: StatusCode::BAD_REQUEST,
                    message: "Bad Request".into(),
                })
            } else {
                Ok(r)
            }
        }
        Err(err) => match err {
            bitwarden::error::Error::NotAuthenticated => Err(ErrorMessage {
                code: StatusCode::UNAUTHORIZED,
                message: "Unauthorized".into(),
            }),
            bitwarden::error::Error::VaultLocked => Err(ErrorMessage {
                code: StatusCode::LOCKED,
                message: "Vault Locked".into(),
            }),
            bitwarden::error::Error::AccessTokenInvalid(e) => Err(ErrorMessage {
                code: StatusCode::UNAUTHORIZED,
                message: e.to_string().into(),
            }),
            bitwarden::error::Error::InvalidResponse => Err(ErrorMessage {
                code: StatusCode::UNPROCESSABLE_ENTITY,
                message: "Invalid Response".into(),
            }),
            bitwarden::error::Error::MissingFields => Err(ErrorMessage {
                code: StatusCode::UNPROCESSABLE_ENTITY,
                message: "Missing Fields".into(),
            }),
            bitwarden::error::Error::Crypto(e) => Err(ErrorMessage {
                code: StatusCode::UNAUTHORIZED,
                message: e.to_string().into(),
            }),
            bitwarden::error::Error::InvalidCipherString(e) => Err(ErrorMessage {
                code: StatusCode::UNAUTHORIZED,
                message: e.to_string().into(),
            }),
            bitwarden::error::Error::IdentityFail(e) => Err(ErrorMessage {
                code: StatusCode::UNAUTHORIZED,
                message: e.to_string().into(),
            }),
            bitwarden::error::Error::Reqwest(e) => {
                error!(error = e.to_string(), "reqwest error");
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
                })
            }
            bitwarden::error::Error::Serde(e) => {
                error!(error = e.to_string(), "serde error");
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
                })
            }
            bitwarden::error::Error::Io(e) => {
                error!(error = e.to_string(), "io error");
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
                })
            }
            bitwarden::error::Error::InvalidBase64(e) => {
                error!(error = e.to_string(), "base64 error");
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
                })
            }
            bitwarden::error::Error::ResponseContent { status, message } => {
                error!(
                    status = status.as_u16(),
                    message = message,
                    "response content error"
                );

                let m = match serde_json::from_str::<ResponseContent>(&message) {
                    Ok(v) => v.message,
                    Err(_) => message,
                };

                Err(ErrorMessage {
                    code: status,
                    message: m,
                })
            }
            bitwarden::error::Error::Internal(e) => {
                error!(error = e, "internal error");
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
                })
            }
        },
    }

    // let reply = match result {
    //     Ok(r) => warp::reply::with_status(warp::reply::json(&r), StatusCode::OK),
    //     Err(e) => e.into(),
    // };

    // Ok(reply)
}

async fn handle_rejection(err: warp::Rejection) -> Result<impl warp::Reply, Infallible> {
    let content;
    if err.is_not_found() {
        content = ErrorMessage {
            code: StatusCode::NOT_FOUND,
            message: "Not Found".into(),
        };
    } else if let Some(e) = err.find::<ErrorMessage>() {
        content = e.clone();
    } else {
        error!(err = format!("{err:?}"), "Unhandled Rejection");
        content = ErrorMessage {
            code: StatusCode::INTERNAL_SERVER_ERROR,
            message: "Internal Server Error".into(),
        };
    }

    Ok(warp::reply::with_status(
        warp::reply::json(&content),
        content.code,
    ))
}

async fn exit_signals() {
    let mut sigint = signal::unix::signal(SignalKind::interrupt()).expect("Failed to bind SIGINT");
    let mut sigterm =
        signal::unix::signal(SignalKind::terminate()).expect("Failed to bind SIGTERM");

    let kind = tokio::select! {
        _ = signal::ctrl_c() => "Ctrl+C",
        _ = sigint.recv() => "SIGINT",
        _ = sigterm.recv() => "SIGTERM",
    };

    info!("Got {kind} - Shutting Down")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);

    let ip = match args.next() {
        Some(a) => IpAddr::from_str(a.as_str())?,
        None => IpAddr::from([127, 0, 0, 1]),
    };
    let port = match args.next() {
        Some(a) => a.parse::<u16>()?,
        None => 3030,
    };

    let format = tracing_subscriber::fmt::format()
        .without_time()
        .with_level(false)
        .with_target(false);
    tracing_subscriber::fmt()
        .event_format(format.pretty().with_source_location(false))
        .with_max_level(tracing::Level::INFO)
        .init();

    let get_secret = warp::get()
        .and(warp::path!(Uuid / Uuid / "secret" / Uuid))
        .and(warp::header::<String>("authorization"))
        .and_then(|org, proj, secret, token| async move {
            match get_secret(org, proj, secret, token).await {
                Ok(r) => Ok(warp::reply::with_status(
                    warp::reply::json(&r),
                    StatusCode::OK,
                )),
                Err(e) => Err(warp::reject::custom(e)),
            }
        });

    let health = warp::get()
        .and(warp::path!("_health"))
        .map(|| warp::reply::with_status("Ok", StatusCode::OK));

    let routes = health.or(get_secret).recover(handle_rejection);

    let (addr, server) = warp::serve(routes)
        .try_bind_with_graceful_shutdown(SocketAddr::from((ip, port)), exit_signals())?;

    info!(address = format!("{addr:?}"), "Listening, ctrl+c to exit");

    server.await;

    Ok(())
}
