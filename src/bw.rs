use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use axum_extra::{
    TypedHeader,
    headers::{authorization::Bearer, Authorization}
};
use bitwarden::{
    auth::login::AccessTokenLoginRequest,
    client::client_settings::{ClientSettings, DeviceType},
    secrets_manager::secrets::{SecretGetRequest, SecretResponse},
    Client,
};
use serde::{ser::SerializeStruct, Serialize};
use tracing::*;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ErrorMessage {
    pub code: StatusCode,
    pub message: String,
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

#[derive(Clone)]
pub struct Settings {
    identity_url: String,
    api_url: String,
}
pub fn settings(identity_url: Option<String>, api_url: Option<String>) -> Settings {
    Settings {
        identity_url: identity_url.unwrap_or("https://identity.bitwarden.com".to_string()),
        api_url: api_url.unwrap_or("https://api.bitwarden.com".to_string()),
    }
}

impl From<Settings> for ClientSettings {
    fn from(val: Settings) -> ClientSettings {
        ClientSettings {
            identity_url: val.identity_url,
            api_url: val.api_url,
            user_agent: "bws_rest_proxy".to_string(),
            device_type: DeviceType::SDK,
        }
    }
}
#[derive(serde::Serialize, Clone)]
pub struct StructuredSecretResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Option<Uuid>,

    pub key: String,
    pub value: serde_json::Value,
    pub note: String,

    pub creation_date: String,
    pub revision_date: String,
}

impl From<SecretResponse> for StructuredSecretResponse {
    fn from(val: SecretResponse) -> Self {
        StructuredSecretResponse {
            id: val.id,
            organization_id: val.organization_id,
            project_id: val.project_id,
            key: val.key,
            value: serde_yaml::from_str(&val.value)
                .unwrap_or(serde_json::Value::String(val.value)),
            note: val.note,
            creation_date: val.creation_date.to_rfc3339(),
            revision_date: val.revision_date.to_rfc3339(),
        }
    }
}

pub async fn get_secret(
    State(settings): State<Settings>,
    Path((org_id, project_id, secret_id)): Path<(Uuid, Uuid, Uuid)>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<StructuredSecretResponse>, ErrorMessage> {
    info!(
        org_id = format!("{org_id:?}"),
        project_id = format!("{project_id:?}"),
        secret_id = format!("{secret_id:?}"),
        "get_secret request"
    );

    let mut client = Client::new(Some(settings.into()));

    let token_request = &AccessTokenLoginRequest {
        access_token: auth.token().to_string(),
        state_file: None,
    };
    if let Err(e) = client.auth().login_access_token(token_request).await {
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
                Ok(Json(r.into()))
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
                message: e.to_string(),
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
                message: e.to_string(),
            }),
            bitwarden::error::Error::InvalidEncString(e) => Err(ErrorMessage {
                code: StatusCode::UNAUTHORIZED,
                message: e.to_string(),
            }),
            bitwarden::error::Error::IdentityFail(e) => Err(ErrorMessage {
                code: StatusCode::UNAUTHORIZED,
                message: e.to_string(),
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
            bitwarden::error::Error::Chrono(e) => {
                error!(error = e.to_string(), "chrono error");
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
                    code: StatusCode::from_u16(status.as_u16()).unwrap(), // This is super annoying due to conflicting dep versions from what I can see
                    message: m,
                })
            }
            bitwarden::error::Error::InvalidStateFileVersion => {
                error!("invalid state file version");
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
                })
            }
            bitwarden::error::Error::InvalidStateFile => {
                error!("invalid state file");
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
                })
            }
            bitwarden::error::Error::Internal(e) => {
                error!(error = e.to_string(), "internal error");
                Err(ErrorMessage {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Internal Server Error".into(),
                })
            }
        },
    }
}
