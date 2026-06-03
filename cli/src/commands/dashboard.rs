//! Dashboard stats command.
//!
//!   - `dashboard [--year <YYYY>]` → GET /api/integrations/dashboard/stats
//!
//! Identity comes from the `eb_live_` Bearer key — no owner id is sent.

use crate::client::ApiClient;
use crate::output;
use anyhow::Result;

/// `easybooks dashboard [--year <YYYY>]`
/// → GET /api/integrations/dashboard/stats
pub fn stats(client: &ApiClient, year: Option<u32>) -> Result<()> {
    let mut q: Vec<(&str, String)> = vec![];
    if let Some(y) = year {
        q.push(("year", y.to_string()));
    }
    output::print_json(&client.get("/api/integrations/dashboard/stats", q)?)
}
