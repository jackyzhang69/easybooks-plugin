---
name: tell-jacky
description: Submit EasyBooks product feedback to Jacky via portal product_feedback.
---

# Tell Jacky (EasyBooks)

Human → Jacky product inbox. Bound `plugin_id=easybooks`.

## Rules

1. Draft in plain language. Show the user title + description before send.
2. Require explicit confirmation. Only then call the CLI with `--user-confirmed`.
3. Never put tokens, keys, bank data, receipts, PDFs, or customer PII in the payload.
4. Requires portal owner token (`jz_`) via `easybooks login --token-stdin`.

## Submit

```bash
easybooks feedback create \
  --title "..." \
  --description "..." \
  --kind bug-report \
  --idempotency-key "<stable-key>" \
  --user-confirmed
```

`kind`: `bug-report` | `feature-request` | `knowledge-tip`

## Status

```bash
easybooks feedback status --report-id <id>
```

Report the returned status only. Do not promise a fix date.
