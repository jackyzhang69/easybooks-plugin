# EasyBooks Plugin — Architecture

This document describes how the EasyBooks plugin moves data from a conversation
into the EasyBooks books, why there is a CLI boundary, the Gmail roadmap, and
how this repo relates to the host app `/Users/jacky/eb`.

The authoritative names, shapes, and rules are in
[`CONTRACT.md`](./CONTRACT.md). This file explains the *why* and the data flow;
the contract is the single source of truth where they overlap.

## Data flow

```
  user drops a receipt / pastes an invoice / points at Gmail
                        |
                        v
  [ agent ]  parses locally (text, image, PDF, email) -> structured Entry JSON
                        |
                        v
  [ easybooks CLI ]  validates, masks the key, calls the backend over HTTP
                        |   POST /api/integrations/ingest/transactions
                        |   POST /api/integrations/ingest/invoice
                        |   GET  /api/integrations/{whoami,categories,clients,invoices}
                        v
  [ eb backend ]  service-role Express handlers, per-user (API-key) scoped, idempotent upsert
                        |
                        v
  [ Supabase ]  eb_transactions / eb_invoices / eb_invoice_items / eb_clients
```

The agent does the *parsing*; the CLI does the *recording*. The two are kept
strictly separate so that the only thing that ever touches EasyBooks data is the
bundled binary.

### Where records land

- Income and expenses go into `eb_transactions`. Rows carry
  `status='completed'`, a `classification` (`business` by default), and the
  provenance triple `(source_system, source_id, source_payload)`.
- Invoices resolve or create a row in `eb_clients`, then insert into
  `eb_invoices` plus its line items in `eb_invoice_items`. Subtotal, tax, and
  total are computed server-side.

### Idempotency

Every write is keyed on provenance so the same source document recorded twice
does not duplicate:

- **Transactions** upsert on `(user_id, source_system, source_id)`. This mirrors
  the existing `POST /api/integrations/audit/stripe-payouts` handler in the host
  app, which already upserts on that exact conflict target.
- **Invoices** upsert / short-circuit on `(user_id, source_system, source_id)`
  when a `source_id` is supplied, otherwise on `(user_id, invoice_number)`.

For Gmail, the `source_id` is the Gmail message id, so re-scanning an inbox never
re-records the same email.

### Identity and scope (per-user API keys)

There is no separate owner id. Each request carries `Authorization: Bearer
<api_key>` — the user's personal EasyBooks API key (`eb_live_…`), generated in the
web app (Settings → API Keys). The backend validates the key and derives the
owning `user_id` and `scope` (`read` or `read_write`) from it; the key IS the
identity. Write endpoints (`ingest/transactions`, `ingest/invoice`,
`invoice/:id/send`) require `read_write`; reads require `read`. An invalid or
missing key is rejected with 401; an under-scoped key with 403. The CLI persists
only the key + base-url at `login` time, so the agent never has to know or guess
the user id.

## Why a CLI boundary

The plugin could, in principle, let the agent call Supabase or the backend
directly. It deliberately does not. A single CLI boundary buys:

- **One auditable surface.** Every EasyBooks mutation flows through one binary
  with one auth header (`Authorization: Bearer <api_key>`), making it easy to
  reason about what can write to the books.
- **Secret containment.** The user's Portal token lives only in
  `~/.jackyzhang.app/token/user.json`. Runtime config is
  `~/.jackyzhang.app/easybooks/config.json`. The agent never sees, prints, or stores
  the token; the CLI masks it anywhere it would surface.
- **No id guessing.** Reads (`categories`, `clients`, `invoices`) let the agent
  resolve human names to the ids the backend expects, instead of inventing them.
- **Stable contract.** Skills, CI, and the backend all conform to the command
  surface and endpoint shapes in `CONTRACT.md`. The agent's job stays "parse the
  document"; the recording semantics live behind the CLI and can evolve without
  re-teaching every skill.
- **Local parsing stays cheap and private.** Reading a PDF or an image happens on
  the user's machine; only the extracted, structured Entry JSON crosses the wire.

## Gmail roadmap: v1 (MCP) → v2 (native OAuth)

- **v1 — Gmail via MCP (this build).** The agent reads candidate receipts and
  invoices through the connected Gmail MCP, extracts them to Entry JSON, and
  records them with `easybooks gmail record` (an alias of `tx import-json` with
  `source_system` defaulted to `gmail`, requiring each entry's `source_id` to be
  the Gmail message id). `easybooks gmail sync` is a deliberate stub in v1 and
  returns `not_implemented_v1` with a hint to use the MCP path.
- **v2 — native OAuth in the CLI (documented, not built).** `gmail sync` will
  pull candidate receipts/invoices headless so the flow can run on a schedule
  (cron) without an interactive agent. The CLI keeps a clear seam for this — a
  dedicated `gmail` command module with a `sync` subcommand already present as a
  stub — so v2 slots in without reshaping the command surface.

## Relationship to the `eb` repo

The backend integration endpoints are **not** in this repo. They live in the
host app at `/Users/jacky/eb`:

- The integration routes (`/api/integrations/whoami`,
  `/api/integrations/ingest/transactions`, `/api/integrations/ingest/invoice`,
  and the `categories`/`clients`/`invoices` reads) are added to the EasyBooks
  Express backend, alongside and modeled on the existing
  `/api/integrations/audit/stripe-payouts` handler.
- Invoice idempotency requires a new migration in the host app,
  `supabase/migrations/009_invoice_external_sources.sql`, adding
  `source_system`, `source_id`, `source_payload` and a unique index
  `(user_id, source_system, source_id)` to `eb_invoices` — mirroring the
  existing external-sources migration for transactions.
- Auth is per-user API keys (no shared env secret). Users mint `eb_live_…` keys in
  the EasyBooks web app (Settings → API Keys) with a `read` or `read_write` scope;
  the backend validates the Bearer key, derives the owning `user_id` + scope from
  it, and enforces scope on each endpoint. The `audit/stripe-payouts` endpoint is
  separate and unchanged.

Per the contract's governance section, the backend code and migration are
**development artifacts**. Migrations are not auto-applied, and nothing here is
deployed to production; applying and deploying are separate, approval-gated
steps.
