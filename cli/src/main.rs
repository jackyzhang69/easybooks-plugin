mod client;
mod commands;
mod config;
mod doctor;
mod output;

// The binary/version resolver lives in the lib crate so it can be unit-tested
// in isolation; re-export the path the binary uses.
use easybooks_cli::bootstrap;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use commands::{gmail, invoices, read, setup, transactions};

#[derive(Parser)]
#[command(name = "easybooks")]
#[command(about = "EasyBooks CLI — the only boundary for EasyBooks reads/writes")]
#[command(version)]
struct Cli {
    /// Override the configured backend base URL for this invocation.
    #[arg(long, global = true)]
    base_url: Option<String>,
    /// Accepted for parity; output is JSON by default. Declared with a distinct
    /// argument id (`json_output`) so it never collides with the per-command
    /// `--json <payload>` args (tx import-json / invoice create / gmail record),
    /// and intentionally NOT `global` so it does not propagate into — and shadow
    /// — those subcommand args. Pass it at the top level (e.g. `easybooks --json
    /// doctor`); it is a no-op since output is already machine-first JSON.
    #[arg(long = "json", id = "json_output", default_value_t = true)]
    json: bool,
    /// Reserved for parity with formbro; currently a no-op (output is already
    /// machine-first JSON / structured errors).
    #[arg(long, global = true, default_value_t = false)]
    quiet: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Persist the user's API key + base-url to ~/.easybooks/config.json.
    Login(LoginArgs),
    /// GET /api/integrations/whoami — confirm config + backend reachability.
    Whoami,
    /// Local config check + backend round-trip + version + cache freshness.
    Doctor(doctor::DoctorArgs),
    /// List categories (resolve names → ids).
    Categories(CategoriesCommand),
    /// List / find clients (resolve names → ids).
    Clients(ClientsCommand),
    /// List invoices.
    Invoices(InvoicesCommand),
    /// Record a single income entry.
    Income(IncomeCommand),
    /// Record a single expense entry.
    Expense(ExpenseCommand),
    /// Transaction batch operations.
    Tx(TxCommand),
    /// Invoice operations (create / send).
    Invoice(InvoiceCommand),
    /// Gmail receipt/invoice recording (v1) + sync stub.
    Gmail(GmailCommand),
}

#[derive(Args)]
struct LoginArgs {
    /// The user's personal EasyBooks API key (`eb_live_...`). It both
    /// authenticates and identifies the user. Never printed.
    #[arg(long)]
    token: String,
    /// Backend base URL. When omitted, resolved at login time through the
    /// documented precedence (contract §6): `--base-url` → `$EASYBOOKS_API_URL`
    /// → DEFAULT (PROD `https://easybooks.jackyzhang.app`). Override for
    /// test/LAN. No clap default so the env tier is honoured instead of silently
    /// persisting PROD.
    #[arg(long)]
    base_url: Option<String>,
}

#[derive(Args)]
struct CategoriesCommand {
    #[command(subcommand)]
    command: CategoriesSub,
}

#[derive(Subcommand)]
enum CategoriesSub {
    /// GET /api/integrations/categories
    List {
        #[arg(long = "type", value_enum)]
        type_filter: Option<TxType>,
    },
}

#[derive(Args)]
struct ClientsCommand {
    #[command(subcommand)]
    command: ClientsSub,
}

#[derive(Subcommand)]
enum ClientsSub {
    /// GET /api/integrations/clients
    List,
    /// GET /api/integrations/clients?query=<q>
    Find {
        #[arg(long)]
        query: String,
    },
}

#[derive(Args)]
struct InvoicesCommand {
    #[command(subcommand)]
    command: InvoicesSub,
}

#[derive(Subcommand)]
enum InvoicesSub {
    /// GET /api/integrations/invoices
    List {
        #[arg(long)]
        status: Option<String>,
    },
}

#[derive(Clone, ValueEnum)]
enum TxType {
    Income,
    Expense,
}

impl TxType {
    fn as_str(&self) -> &'static str {
        match self {
            TxType::Income => "income",
            TxType::Expense => "expense",
        }
    }
}

#[derive(Clone, ValueEnum)]
enum Classification {
    Business,
    Personal,
}

impl Classification {
    fn as_str(&self) -> &'static str {
        match self {
            Classification::Business => "business",
            Classification::Personal => "personal",
        }
    }
}

/// Shared flag set for `income add` / `expense add`.
#[derive(Args)]
struct EntryAddArgs {
    #[arg(long)]
    amount: String,
    #[arg(long)]
    description: String,
    #[arg(long)]
    date: String,
    #[arg(long)]
    category: Option<String>,
    #[arg(long, value_enum)]
    classification: Option<Classification>,
    #[arg(long = "source-system")]
    source_system: Option<String>,
    #[arg(long = "source-id")]
    source_id: Option<String>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long = "dry-run", default_value_t = false)]
    dry_run: bool,
}

impl EntryAddArgs {
    fn into_add_args(self) -> transactions::AddArgs {
        transactions::AddArgs {
            amount: self.amount,
            description: self.description,
            date: self.date,
            category: self.category,
            classification: self.classification.map(|c| c.as_str().to_string()),
            source_system: self.source_system,
            source_id: self.source_id,
            notes: self.notes,
            dry_run: self.dry_run,
        }
    }
}

#[derive(Args)]
struct IncomeCommand {
    #[command(subcommand)]
    command: IncomeSub,
}

#[derive(Subcommand)]
enum IncomeSub {
    /// Record one income entry via POST /api/integrations/ingest/transactions.
    Add(EntryAddArgs),
}

#[derive(Args)]
struct ExpenseCommand {
    #[command(subcommand)]
    command: ExpenseSub,
}

#[derive(Subcommand)]
enum ExpenseSub {
    /// Record one expense entry via POST /api/integrations/ingest/transactions.
    Add(EntryAddArgs),
}

#[derive(Args)]
struct TxCommand {
    #[command(subcommand)]
    command: TxSub,
}

#[derive(Subcommand)]
enum TxSub {
    /// Batch ingest: --json '{ source_system, entries:[Entry] }'.
    ImportJson {
        #[arg(long)]
        json: String,
        #[arg(long = "dry-run", default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Args)]
struct InvoiceCommand {
    #[command(subcommand)]
    command: InvoiceSub,
}

#[derive(Subcommand)]
enum InvoiceSub {
    /// Create an invoice via POST /api/integrations/ingest/invoice.
    Create {
        #[arg(long)]
        json: String,
        #[arg(long = "dry-run", default_value_t = false)]
        dry_run: bool,
    },
    /// Send an invoice/receipt email via the legacy backend.
    Send {
        /// The invoice id.
        invoice_id: String,
    },
}

#[derive(Args)]
struct GmailCommand {
    #[command(subcommand)]
    command: GmailSub,
}

#[derive(Subcommand)]
enum GmailSub {
    /// Record agent-extracted Gmail receipts/invoices (source_system=gmail,
    /// source_id = Gmail message id). Alias of `tx import-json`.
    Record {
        #[arg(long)]
        json: String,
        #[arg(long = "dry-run", default_value_t = false)]
        dry_run: bool,
    },
    /// v1 STUB — native OAuth sync ships in v2.
    Sync,
}

fn main() {
    if let Err(error) = run() {
        output::print_error(&error);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let base_url_arg = cli.base_url;
    // `--json` / `--quiet` exist for parity with formbro's global flags. Output
    // is already machine-first JSON and errors are structured, so they are
    // accepted but do not alter behaviour in v1. Bind them so the parsed fields
    // are observed (keeps `-D warnings` clean) and the intent is documented.
    let _parity_flags = (cli.json, cli.quiet);

    // `login` and `doctor` are the two commands that do NOT require an already
    // authenticated client: `login` writes the config; `doctor` tolerates a
    // missing config and runs its own scoped (optional) network probe. Handle
    // them first so they work before/without a successful `Config::load`.
    match cli.command {
        Command::Login(args) => setup::login(&args.token, args.base_url),
        Command::Doctor(args) => doctor::run(args, base_url_arg, None),
        other => dispatch(other, base_url_arg),
    }
}

/// Build the authenticated client from config and dispatch every command that
/// talks to the backend. Split out so `run` can short-circuit `login`/`doctor`.
fn dispatch(command: Command, base_url_arg: Option<String>) -> Result<()> {
    let config = config::Config::load(base_url_arg, None)?;
    let client = client::ApiClient::new(config.base_url.clone(), config.api_key.clone())?;

    match command {
        Command::Login(_) | Command::Doctor(_) => unreachable!(),
        Command::Whoami => setup::whoami(&client, &config),
        Command::Categories(cmd) => match cmd.command {
            CategoriesSub::List { type_filter } => {
                read::categories_list(&client, type_filter.map(|t| t.as_str().to_string()))
            }
        },
        Command::Clients(cmd) => match cmd.command {
            ClientsSub::List => read::clients_list(&client),
            ClientsSub::Find { query } => read::clients_find(&client, query),
        },
        Command::Invoices(cmd) => match cmd.command {
            InvoicesSub::List { status } => read::invoices_list(&client, status),
        },
        Command::Income(cmd) => match cmd.command {
            IncomeSub::Add(args) => transactions::add(&client, "income", args.into_add_args()),
        },
        Command::Expense(cmd) => match cmd.command {
            ExpenseSub::Add(args) => transactions::add(&client, "expense", args.into_add_args()),
        },
        Command::Tx(cmd) => match cmd.command {
            TxSub::ImportJson { json, dry_run } => {
                transactions::import_json(&client, &json, dry_run)
            }
        },
        Command::Invoice(cmd) => match cmd.command {
            InvoiceSub::Create { json, dry_run } => invoices::create(&client, &json, dry_run),
            InvoiceSub::Send { invoice_id } => invoices::send(&client, &invoice_id),
        },
        Command::Gmail(cmd) => match cmd.command {
            GmailSub::Record { json, dry_run } => gmail::record(&client, &json, dry_run),
            GmailSub::Sync => gmail::sync(),
        },
    }
}

// Keep the resolver reachable from the binary (used by skills' §B blocks at
// runtime and surfaced via doctor's cache check). Referencing it here also
// documents the §0 contract entry point for maintainers.
#[allow(dead_code)]
fn _resolver_entrypoints() {
    let _ = bootstrap::resolve_binary;
    let _ = bootstrap::current_platform;
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn parses_income_add() {
        let cli = Cli::try_parse_from([
            "easybooks",
            "income",
            "add",
            "--amount",
            "120.00",
            "--description",
            "Consulting",
            "--date",
            "2026-05-01",
        ])
        .expect("income add should parse");
        match cli.command {
            super::Command::Income(_) => {}
            _ => panic!("expected income command"),
        }
    }

    #[test]
    fn parses_tx_import_json_dry_run() {
        let cli = Cli::try_parse_from([
            "easybooks",
            "tx",
            "import-json",
            "--json",
            r#"{"source_system":"manual","entries":[]}"#,
            "--dry-run",
        ])
        .expect("tx import-json should parse");
        match cli.command {
            super::Command::Tx(_) => {}
            _ => panic!("expected tx command"),
        }
    }

    #[test]
    fn parses_gmail_sync() {
        let cli = Cli::try_parse_from(["easybooks", "gmail", "sync"])
            .expect("gmail sync should parse");
        match cli.command {
            super::Command::Gmail(_) => {}
            _ => panic!("expected gmail command"),
        }
    }
}
