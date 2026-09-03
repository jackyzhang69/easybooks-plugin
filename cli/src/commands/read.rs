//! Read commands (contract §2 "Reads") so the agent can resolve names → ids and
//! never guess. All hit service-role, owner-scoped GET endpoints (§3).

use crate::client::ApiClient;
use crate::output;
use anyhow::{anyhow, Result};
use serde_json::json;

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

/// `easybooks categories create --name <n> --type <income|expense> [--tax-deductible]`
/// → POST /api/integrations/categories
pub fn categories_create(
    client: &ApiClient,
    name: &str,
    type_: &str,
    tax_deductible: bool,
) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("--name must not be empty"));
    }
    validate_category_type(type_)?;
    let mut body = serde_json::Map::new();
    body.insert("name".to_string(), json!(name));
    body.insert("type".to_string(), json!(type_));
    if tax_deductible {
        body.insert("tax_deductible".to_string(), json!(true));
    }
    output::print_json(&client.post(
        "/api/integrations/categories",
        &serde_json::Value::Object(body),
    )?)
}

fn validate_category_type(t: &str) -> Result<()> {
    match t {
        "income" | "expense" => Ok(()),
        _ => Err(anyhow!("--type must be income|expense (got {:?})", t)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_type_validation() {
        assert!(validate_category_type("income").is_ok());
        assert!(validate_category_type("expense").is_ok());
        assert!(validate_category_type("Income").is_err());
        assert!(validate_category_type("asset").is_err());
        assert!(validate_category_type("").is_err());
    }
}
