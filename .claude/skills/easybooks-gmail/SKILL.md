---
name: easybooks-gmail
description: Scan Gmail for receipts and invoices and record them into EasyBooks. v1 — the agent reads candidate messages via the connected Gmail MCP, extracts each to Entry JSON with source_id = the Gmail message id (so re-scans never double-record), then records with `easybooks gmail record`. Native OAuth sync (`gmail sync`) is a v2 stub today. Requires connect-easybooks first. See easybooks-capabilities for the full router and JSON shapes.
when_to_use: |-
  Trigger phrases:
    - "scan my Gmail / inbox for receipts / invoices and record them"
    - "find purchase receipts in my email and add them to EasyBooks"
    - "import my email receipts into the books"
    - "go through my Gmail and log the expenses"
---

# EasyBooks ↔ Gmail (v1)

All recording goes through the bundled `easybooks` CLI. Resolve it once via `easybooks-capabilities/SKILL.md` §B and invoke that exact path; do not rely on ambient `PATH`.

## How v1 works (read this first)

There is **no native Gmail OAuth in the CLI yet**. In v1 the split is:

1. **The agent reads Gmail via the connected Gmail MCP.** You search the inbox, open candidate messages, and read their bodies/attachments locally to extract the transaction facts. The CLI does not touch Gmail.
2. **The CLI records what you extracted** via `easybooks gmail record` — an alias of `tx import-json` with `source_system` defaulted to `gmail`. The boundary rule still holds: parse locally, record through the CLI.

If the Gmail MCP is **not** connected, tell the user to connect it first (the Gmail integration in their agent), then retry. Do not attempt to read mail any other way.

## Idempotency — source_id MUST be the Gmail message id

This is the whole reason re-scanning your inbox is safe:

- For **every** entry, set `source_id` to the **Gmail message id** of the email it came from.
- Recorded rows are upserted on `(user_id, source_system, source_id)` with `source_system = "gmail"`. So scanning the same inbox window twice records each message **once** — the second run reports those rows under `existing`, not `created`.
- Never substitute a random id, a running counter, or a hash that changes per run for `source_id` — that defeats idempotency and double-records. The message id is stable and unique; use it.

## Step 1 — find candidate receipts/invoices in Gmail

Use the Gmail MCP's search with a query targeting purchase receipts and invoices. Copy-pasteable starting query (tune the date window to the user's request):

```
(subject:(receipt OR invoice OR "order confirmation" OR "payment received" OR "your receipt" OR "tax invoice")
  OR label:receipts OR label:invoices OR category:purchases)
  -in:spam -in:trash newer_than:90d
```

Notes:
- Narrow with `newer_than:30d` / `after:2026/05/01 before:2026/06/01` to match the period the user asked for. Don't scan the whole mailbox unless asked.
- Add vendor hints if the user names them, e.g. `from:(stripe.com OR adobe.com OR amazon.ca)`.
- Common false positives: marketing "invoices you'll love", newsletters with "receipt" in subject. Filter these out when you read the body — only record real transactions.

## Step 2 — read each candidate and extract to Entry JSON

For each promising message, open it via the Gmail MCP, read the body (and parse any PDF/image attachment the same way `easybooks-record` describes), and build one Entry. Capture the **Gmail message id** as `source_id`.

```json
{
  "source_system": "gmail",
  "entries": [
    {
      "type": "expense",
      "amount_cents": 4599,
      "description": "Adobe Creative Cloud — monthly",
      "date": "2026-05-12",
      "category_name": "Software",
      "classification": "business",
      "source_type": "email",
      "source_id": "<gmail-message-id>",
      "source_payload": { "from": "billing@adobe.com", "subject": "Your receipt", "gmail_thread_id": "<id>" }
    }
  ]
}
```

- `amount_cents` is an integer (cents): $45.99 → `4599`.
- `type` is usually `expense` for purchase receipts; an income receipt (a payment you received) is `income`. Decide per message.
- `category_name` optional (backend resolves the name). `classification` defaults to `business`; flag anything that looks personal and ask.
- One message = one Entry as a rule. If a single email contains several distinct charges, you may emit multiple entries but they then need distinct, stable `source_id`s (e.g. `<message-id>#1`, `<message-id>#2`) — keep them deterministic.

## Step 3 — dry-run, confirm, record

```sh
# 3a. Validate + echo resolved rows; writes nothing.
"$EASYBOOKS_BIN" gmail record --json '<json>' --dry-run
# 3b. Show the user the list (vendor, amount, date, category, type). Confirm — especially amounts and business/personal.
# 3c. Record for real (same JSON, no --dry-run):
"$EASYBOOKS_BIN" gmail record --json '<json>'
```

Output is `{ "created":n, "existing":n, "skipped":n, "processed":n }`. On a re-scan, expect `created: 0` and `existing` = the count already recorded — **that is success** (idempotency working), not a failure to fix.

`easybooks gmail record` is exactly `tx import-json` with `source_system` defaulted to `gmail`; you may also call `tx import-json` directly as long as `source_system` is `gmail` and each `source_id` is the message id.

## Do NOT use `gmail sync` in v1

```sh
"$EASYBOOKS_BIN" gmail sync
# → {"status":"not_implemented_v1","hint":"In v1, read Gmail via the Gmail MCP and record with `easybooks gmail record`. Native OAuth sync ships in v2."}
```

`gmail sync` is a **stub** in v1. It does not read mail. If the user asks to "auto-sync" or "set up a cron to pull receipts", explain that v1 is agent-driven (this skill) and native OAuth sync is the v2 path.

## v2 — coming (documented, not built now)

In v2 the CLI gains native Gmail OAuth: `gmail sync` will pull candidate receipts/invoices headless (suitable for a scheduled/cron job) without an agent in the loop. The idempotency contract is unchanged — it will still key on the Gmail message id, so v1 records and v2 records will not collide. There is nothing to configure for v2 today.

## Quick router (user intent → action)

| If the user says… | Do |
|---|---|
| "scan my Gmail for receipts and record them" | Gmail MCP search (§1) → extract per message (§2) → `gmail record --json '<json>' --dry-run` → confirm → rerun without `--dry-run` |
| "just this week's receipts" | same, but tighten the query (`newer_than:7d` / `after:.. before:..`) |
| "auto-sync my email receipts on a schedule" | explain v1 is agent-driven; `gmail sync` is a v2 stub today |

## Default behavior

- Always `--dry-run` first and show the user the extracted rows before recording — email parsing is error-prone (marketing vs real receipts, wrong totals).
- When unsure whether a message is a real receipt, skip it and tell the user, rather than recording a guess.
- Ask one specific question on ambiguous category/classification rather than defaulting silently.

## Governance

- The CLI **defaults to the PROD backend** (`https://easybooks.jackyzhang.app`, the immicore eb-plugin via the eb frontend nginx `/api` proxy); the legacy Node `http://localhost:8080` is no longer the default. Override to test (`https://easybooks-test.jackyzhang.app`) or LAN (`http://192.168.1.98:8310`) via `--base-url`. Because the default is production, recording is a production write and requires an approval artifact (see `easybooks-capabilities` §G). If you'd be writing to production without one, stop and tell the user.
- Recording requires a **read_write** API key. If the CLI returns a scope/permission error, the user's key is read-only — tell them to create a Read & write key in the EasyBooks web app (Settings → API Keys).
- Never print the user's API key; it is masked as `eb_***` and lives only in `~/.easybooks/config.json`. The Gmail MCP's own credentials are separate and managed by that integration — don't echo those either.
