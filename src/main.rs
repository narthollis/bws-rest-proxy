use bitwarden::{
    auth::request::AccessTokenLoginRequest,
    client::client_settings::{ClientSettings, DeviceType},
    secrets_manager::secrets::SecretGetRequest,
    Client,
};
use serde::ser::{Serialize, SerializeStruct};
use std::convert::Infallible;
use tracing::*;
use uuid::Uuid;
use warp::{hyper::StatusCode, Filter};

struct ErrorMessage {
    code: StatusCode,
    message: String,
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

async fn get_secret(
    org_id: Uuid,
    project_id: Uuid,
    secret_id: Uuid,
    access_token: String,
) -> Result<impl warp::Reply, Infallible> {
    info!(
        org_id = format!("{org_id:?}"),
        project_id = format!("{project_id:?}"),
        secret_id = format!("{secret_id:?}"),
        "get_secret request"
    );

    let settings = ClientSettings {
        identity_url: "https://identity.bitwarden.com".to_string(),
        api_url: "https://api.bitwarden.com".to_string(),
        user_agent: "Bitwarden Rust-SDK".to_string(),
        device_type: DeviceType::SDK,
    };
    let mut client = Client::new(Some(settings));

    match client
        .access_token_login(&AccessTokenLoginRequest {
            access_token: access_token,
        })
        .await
    {
        Ok(r) => trace!(login_response = format!("{r:?}"), "login successful"),
        Err(e) => error!(login_error = format!("{e:?}"), "login error"),
    }

    let request = SecretGetRequest { id: secret_id };

    let result = match client.secrets().get(&request).await {
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
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
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
    };

    let reply = match result {
        Ok(r) => warp::reply::with_status(warp::reply::json(&r), StatusCode::OK),
        Err(e) => warp::reply::with_status(warp::reply::json(&e), e.code),
    };

    Ok(reply)
}

#[tokio::main]
async fn main() {
    let format = tracing_subscriber::fmt::format()
        .without_time()
        .with_level(false)
        .with_target(false);
    tracing_subscriber::fmt()
        .event_format(format.pretty().with_source_location(false))
        .with_max_level(tracing::Level::INFO)
        .init();

    // GET /hello/warp => 200 OK with body "Hello, warp!"
    let get_secret = warp::get()
        .and(warp::path!(Uuid / Uuid / "secret" / Uuid))
        .and(warp::header::<String>("authorization"))
        .and_then(get_secret);

    warp::serve(get_secret).run(([127, 0, 0, 1], 3030)).await;
}
