//! Read commands (contract §2 "Reads") so the agent can resolve names → ids and
//! never guess. All hit service-role, owner-scoped GET endpoints (§3).

use crate::client::ApiClient;
use crate::output;
use anyhow::Result;

/// `easybooks categories list [--type income|expense]`
/// → GET /api/integrations/categories
pub fn categories_list(client: &ApiClient, type_filter: Option<String>) -> Result<()> {
    let mut query = vec![];
    if let Some(t) = type_filter {
        query.push(("type", t));
    }
    output::print_json(&client.get("/api/integrations/categories", query)?)
}

/// `easybooks clients list` → GET /api/integrations/clients (no query)
pub fn clients_list(client: &ApiClient) -> Result<()> {
    output::print_json(&client.get("/api/integrations/clients", vec![])?)
}

/// `easybooks clients find --query <q>` → GET /api/integrations/clients?query=<q>
pub fn clients_find(client: &ApiClient, query_str: String) -> Result<()> {
    let query = vec![("query", query_str)];
    output::print_json(&client.get("/api/integrations/clients", query)?)
}

/// `easybooks invoices list [--status <s>]` → GET /api/integrations/invoices
pub fn invoices_list(client: &ApiClient, status: Option<String>) -> Result<()> {
    let mut query = vec![];
    if let Some(s) = status {
        query.push(("status", s));
    }
    output::print_json(&client.get("/api/integrations/invoices", query)?)
}
