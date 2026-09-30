//! `sfctl`: Kommandozeile für die STARFACE, auf demselben Kern wie die
//! Desktop-App. Dient vorerst als Entwicklungs- und Testwerkzeug.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use sf_onehub::sf_proto::v1;
use sf_onehub::{OneHub, TokenHandle};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Adresse der Anlage, z. B. https://pbx.example.com
    #[arg(long, env = "SF_SERVER")]
    server: String,
    /// Port der OneHub-gRPC-API
    #[arg(long, env = "SF_GRPC_PORT", default_value_t = sf_onehub::DEFAULT_PORT)]
    grpc_port: u16,
    /// Access-Token; ohne Token wird per Password-Grant angemeldet
    #[arg(long, env = "SF_TOKEN", hide_env_values = true)]
    token: Option<String>,
    #[arg(long, env = "SF_USER")]
    user: Option<String>,
    #[arg(long, env = "SF_PASSWORD", hide_env_values = true)]
    password: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Version der Anlage
    Version,
    /// Eigenes Profil
    Me,
    /// Eigene Telefone
    Phones,
    /// Anruf starten; die Anlage ruft zuerst das gewählte Telefon an
    Call {
        number: String,
        /// Telefon-ID (siehe `sfctl phones`), sonst das Primärtelefon
        #[arg(long)]
        phone_id: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let host = url::Url::parse(&cli.server)
        .context("SF_SERVER muss eine URL sein, z. B. https://pbx.example.com")?
        .host_str()
        .context("SF_SERVER enthält keinen Hostnamen")?
        .to_owned();

    let token = match (&cli.token, &cli.user, &cli.password) {
        (Some(t), _, _) => t.clone(),
        (None, Some(u), Some(p)) => {
            let auth = sf_auth::Client::discover(&cli.server).await?;
            auth.password_grant(u, p).await?.access_token
        }
        _ => bail!("SF_TOKEN oder SF_USER und SF_PASSWORD setzen"),
    };

    let hub = OneHub::connect(&host, cli.grpc_port, TokenHandle::new(token)).await?;

    match cli.command {
        Command::Version => println!("{}", hub.server_version().await?),
        Command::Me => {
            let user = hub
                .me()
                .get_user(())
                .await?
                .into_inner()
                .user
                .unwrap_or_default();
            println!("{} {} <{}>", user.first_name, user.last_name, user.email);
            for n in user.phone_numbers {
                println!("  {n:?}");
            }
        }
        Command::Phones => {
            let primary = hub.me().get_primary_phone(()).await?.into_inner().phone;
            let primary_id = primary.and_then(|p| p.phone_id).map(|id| id.id);
            for phone in hub.me().get_phones(()).await?.into_inner().phones {
                let id = phone.phone_id.map(|id| id.id).unwrap_or_default();
                let mark = if Some(&id) == primary_id.as_ref() {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {id:>6}  {}", phone.name);
            }
        }
        Command::Call { number, phone_id } => {
            let req = v1::call::PlaceCallRequest {
                number,
                requested_call_id: None,
                phone_id: phone_id.map(|id| v1::types::PhoneId { id }),
            };
            let call = hub.call().place_call(req).await?.into_inner();
            println!("{}", call.call_id.map(|c| c.id).unwrap_or_default());
        }
    }
    Ok(())
}
