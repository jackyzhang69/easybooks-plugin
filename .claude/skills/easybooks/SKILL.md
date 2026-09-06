---
name: easybooks
description: >-
  READ THIS FIRST for EasyBooks. Bookkeeping for self-employed Canadians:
  record this receipt, log a $X expense/income, I dropped a file / invoice /
  screenshot, here's a receipt / supplier invoice / screenshot / scanned PDF —
  record it, import this spreadsheet / bank statement / CSV of expenses,
  what categories do I have, create an invoice, send invoice X, bill a client,
  scan my Gmail for receipts, find purchase receipts in my email, import my
  email receipts, go through my Gmail and log the expenses, list my
  categories/clients/invoices, connect EasyBooks, log in / save my jz_
  platform token, tell Jacky, connect with Jacky, join code from Jacky,
  pair session. One discovery file; playbooks in references/.
  Ask the live CLI (`easybooks commands --json`), never from memory of an
  older release.
when_to_use: |-
  Load on plugin start; reload whenever a user asks anything EasyBooks-related.
  Trigger phrases: "record this receipt", "log a $X expense/income",
  "I dropped a file / invoice / screenshot", "create an invoice",
  "send invoice X", "scan my Gmail for receipts",
  "list my categories/clients/invoices",
  "is EasyBooks healthy / which backend am I on", "connect EasyBooks",
  "connect to easybooks / set up easybooks",
  "log in to easybooks / save my jz_ platform token",
  "configure the easybooks plugin / use this key + base url",
  "scan my Gmail / inbox for receipts / invoices and record them",
  "find purchase receipts in my email and add them to EasyBooks",
  "import my email receipts into the books",
  "go through my Gmail and log the expenses",
  "create / draft / make an invoice for <client> for <items>",
  "bill <client> $X for <work>",
  "send invoice <id> / email this invoice / email the receipt",
  "list my invoices / which invoices are unpaid / find client <name>",
  "log / record a $X expense for <thing> on <date>",
  "record this income / payment I received",
  "here's a receipt / supplier invoice / screenshot / scanned PDF — record it",
  "import this spreadsheet / bank statement / CSV of expenses",
  "what categories do I have", "tell Jacky",
  "connect with Jacky", "join code from Jacky", "pair session".
---

# EasyBooks plugin — agent consumption contract

## Talk to the human (mandatory)

Speak in everyday words. Say what you are doing and what happens next. Do not
lead with binary paths, `--help`, or raw JSON. Keep CLI and JSON between
tools. Never show credentials, bank data, or receipt contents in chat unless
the user asked for that detail. On the first session after install, read
[references/get-started.md](references/get-started.md) before your first
user-visible reply. After a version update, read
[references/whats-new.md](references/whats-new.md) and resume the prior intent
without asking the human to repeat it. Never ask the human to run doctor as
homework. Run the intended ordinary command first; when stdout is
`jz.plugin.envelope.v1`, follow its exact `status` and `continue_args` until
the product reports `ready`, then resume the sealed request. Never assign
doctor homework when an envelope already names the route.

## Live CLI surface (fail-closed)

Ask the live CLI, never from memory of an older release:

```bash
"$EASYBOOKS_BIN" commands --json
```

## Agent router — intents

| User intent | Host does | Human may be asked |
|---|---|---|
| "what can EasyBooks do" | Live `"$EASYBOOKS_BIN" commands --json`; translate record/invoice/Gmail capabilities | Connect once if not logged in ([connect](references/connect.md)) |
| record this receipt / log expense or income / import spreadsheet or statement | [record](references/record.md): parse locally, dry-run import, confirm, write | Amount, date, business vs personal, category when ambiguous |
| create / send invoice / list clients or invoices | [invoice](references/invoice.md) | Client and line items; confirm before `invoice send` emails a client |
| scan Gmail for receipts | [gmail](references/gmail.md) | Confirm parsed rows before write |
| list categories / clients / invoices / dashboard | Live list/read commands per [command-router](references/command-router.md) | Clarify filters when ambiguous |
| connect EasyBooks / save platform token | [connect](references/connect.md): agent pipes token via stdin | Token file path or one-time paste (never argv) |
| tell Jacky | [tell-jacky](references/tell-jacky.md) | Confirm exact draft before send |
| connect with Jacky / pair session / join code from Jacky | [pair-session](references/pair-session.md) | Confirm once that Jacky's assistant may look at this machine's EasyBooks status |

Playbooks: [connect](references/connect.md), [record](references/record.md),
[invoice](references/invoice.md), [gmail](references/gmail.md),
[tell-jacky](references/tell-jacky.md),
[pair-session](references/pair-session.md).

Command router and CLI paths (not for first-session orientation):
[references/command-router.md](references/command-router.md).

EasyBooks is a self-employed (Canadian) finance app: income/expense transactions,
categories, clients, and invoices. This plugin lets an agent record bookkeeping
through one bundled CLI instead of touching the database or backend directly.

## Binary

Prefer `$EASYBOOKS_BIN` if it points at a runnable `easybooks` binary. Otherwise
use the canonical current package
`~/.jackyzhang.app/plugins/easybooks/current/bin/<platform>/easybooks`
(`easybooks.exe` on `win32-x64`). `command -v easybooks` / PATH is last resort
only. Export `EASYBOOKS_BIN` once per session; every later command uses that path.

## Operating rules

1. **All EasyBooks operations go through the bundled CLI.** Do not call the
   backend or write a database directly. Local file parsing is allowed only
   before the CLI boundary.
2. **Never guess ids.** Record with names, or list first
   (`categories list`, `clients find`, `invoices list`).
3. **Money path is dry-run → confirm → write.** Document imports and invoices
   always `--dry-run` first. Confirm amounts and business vs personal. Then
   rerun without `--dry-run`. `invoice send` emails a client — confirm once.
4. **Idempotency is mandatory for recorded documents.** Every parsed receipt /
   invoice / email row carries a stable `source_id`. Re-running the same import
   must not double-record. For Gmail, `source_id` is the Gmail message id.
5. **One shared platform token.** Login is `login --token-stdin` only. The
   durable credential is `~/.jackyzhang.app/token/user.json`. Do not create a
   second durable token. Never print the raw value. Connect details:
   [connect](references/connect.md).
6. **Production is the default.** Writes are gated. Follow the current
   platform-vault project card before a production mutation.

Entry JSON, invoice JSON, classification, and auto-categorization rules live in
the playbooks above — do not invent fields. When the CLI returns a structured
error with a `hint`, surface it verbatim.
