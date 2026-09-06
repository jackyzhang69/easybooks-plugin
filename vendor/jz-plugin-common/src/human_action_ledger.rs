//! Primary-operation work classification ledger (`jz.plugin.human_action_ledger.v1`).

use crate::envelope::{MaterialClass, Violation};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const HUMAN_ACTION_LEDGER_SCHEMA: &str = "jz.plugin.human_action_ledger.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceKind {
    ReadyMachine,
    UnreadyMachine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Machine,
    Human,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineKind {
    HostDiscovery,
    Validation,
    ProductConfig,
    Exchange,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanKind {
    GiveFact,
    Confirm,
    Verify,
    Consent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeStatus {
    Ready,
    NeedsAgent,
    NeedsHuman,
    Blocked,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Kind {
    Machine(MachineKind),
    Human(HumanKind),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub step_id: String,
    pub operation: String,
    pub actor: Actor,
    pub kind: Kind,
    pub envelope_status: EnvelopeStatus,
    pub say_to_user_present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_class: Option<MaterialClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub schema: String,
    pub product: String,
    pub trace_kind: TraceKind,
    pub primary_operation: String,
    pub host_os: HostOs,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostOs {
    Macos,
    Windows,
}

impl Ledger {
    pub fn validate(&self) -> Result<(), Vec<Violation>> {
        let mut violations = Vec::new();

        if self.schema != HUMAN_ACTION_LEDGER_SCHEMA {
            violations.push(viol(
                "invalid-schema",
                "schema must be jz.plugin.human_action_ledger.v1",
            ));
        }
        if self.product.trim().is_empty() {
            violations.push(viol("missing-product", "product is required"));
        }
        if self.primary_operation.trim().is_empty() {
            violations.push(viol(
                "missing-primary-operation",
                "primary_operation is required",
            ));
        }
        if self.rows.is_empty() {
            violations.push(viol("empty-rows", "rows must be non-empty"));
        }

        let mut seen_step_ids = std::collections::HashSet::new();
        for row in &self.rows {
            if row.step_id.trim().is_empty() {
                violations.push(viol("missing-step-id", "row step_id is required"));
            } else if !seen_step_ids.insert(row.step_id.clone()) {
                violations.push(viol(
                    "duplicate-step-id",
                    format!("duplicate step_id {}", row.step_id),
                ));
            }
            if row.operation.trim().is_empty() {
                violations.push(viol(
                    "missing-row-operation",
                    format!("row {} operation is required", row.step_id),
                ));
            }

            validate_actor_kind_pair(row, &mut violations);
            validate_material_class(row, &mut violations);
            validate_envelope_status(row, &mut violations);
        }

        match self.trace_kind {
            TraceKind::ReadyMachine => validate_ready_machine(&self.rows, &mut violations),
            TraceKind::UnreadyMachine => validate_unready_machine(&self.rows, &mut violations),
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }
}

fn validate_actor_kind_pair(row: &Row, violations: &mut Vec<Violation>) {
    match (row.actor, &row.kind) {
        (Actor::Machine, Kind::Machine(_)) => {}
        (Actor::Human, Kind::Human(_)) => {}
        (Actor::Machine, Kind::Human(_)) => {
            violations.push(viol(
                "actor-kind-mismatch",
                format!(
                    "row {} actor=machine requires a machine kind",
                    row.step_id
                ),
            ));
        }
        (Actor::Human, Kind::Machine(_)) => {
            violations.push(viol(
                "actor-kind-mismatch",
                format!("row {} actor=human requires a human kind", row.step_id),
            ));
        }
    }
}

fn validate_material_class(row: &Row, violations: &mut Vec<Violation>) {
    let is_consent = matches!(
        (&row.actor, &row.kind),
        (Actor::Human, Kind::Human(HumanKind::Consent))
    );
    match (is_consent, &row.material_class) {
        (true, None) => violations.push(viol(
            "consent-missing-material-class",
            format!("row {} kind=consent requires material_class", row.step_id),
        )),
        (false, Some(_)) => violations.push(viol(
            "material-class-without-consent",
            format!(
                "row {} material_class is only allowed for kind=consent",
                row.step_id
            ),
        )),
        _ => {}
    }
}

fn validate_envelope_status(row: &Row, violations: &mut Vec<Violation>) {
    match (row.envelope_status, row.actor, &row.kind) {
        (EnvelopeStatus::NeedsHuman, Actor::Human, Kind::Human(HumanKind::Consent)) => {}
        (EnvelopeStatus::NeedsHuman, Actor::Machine, _) => {}
        (EnvelopeStatus::NeedsHuman, Actor::Human, _) => {
            violations.push(viol(
                "needs-human-envelope-mismatch",
                format!(
                    "row {} envelope_status=needs_human requires human kind=consent or machine actor",
                    row.step_id
                ),
            ));
        }
        (EnvelopeStatus::Ready, _, _) | (EnvelopeStatus::NeedsAgent, _, _) => {}
        (EnvelopeStatus::Blocked, _, _) => {}
        (EnvelopeStatus::None, _, _) => {}
    }
}

fn validate_ready_machine(rows: &[Row], violations: &mut Vec<Violation>) {
    for row in rows {
        if row.actor == Actor::Human {
            violations.push(viol(
                "ready-machine-human-row",
                format!(
                    "ready_machine forbids human row at step {}",
                    row.step_id
                ),
            ));
        }
        if row.say_to_user_present {
            violations.push(viol(
                "ready-machine-say-to-user",
                format!(
                    "ready_machine forbids say_to_user_present at step {}",
                    row.step_id
                ),
            ));
        }
    }
}

fn validate_unready_machine(rows: &[Row], violations: &mut Vec<Violation>) {
    for row in rows {
        if row.actor != Actor::Human {
            continue;
        }
        let allowed = matches!(
            row.kind,
            Kind::Human(HumanKind::GiveFact)
                | Kind::Human(HumanKind::Confirm)
                | Kind::Human(HumanKind::Verify)
                | Kind::Human(HumanKind::Consent)
        );
        if !allowed {
            violations.push(viol(
                "unready-machine-invalid-human-kind",
                format!(
                    "unready_machine human rows only allow give_fact, confirm, verify, consent at step {}",
                    row.step_id
                ),
            ));
        }
    }
}

fn viol(code: &'static str, message: impl Into<String>) -> Violation {
    Violation {
        code,
        message: message.into(),
    }
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/fixtures/human-action-ledger-v1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn violation_codes(result: Result<(), Vec<Violation>>) -> Vec<String> {
        result
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.code.to_string())
            .collect()
    }

    fn sample_row(step_id: &str, actor: Actor, kind: Kind) -> Row {
        Row {
            step_id: step_id.into(),
            operation: "primary_action".into(),
            actor,
            kind,
            envelope_status: EnvelopeStatus::None,
            say_to_user_present: false,
            material_class: None,
            note: None,
        }
    }

    #[test]
    fn contract_fixtures_table_driven() {
        let dir = fixtures_dir();
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("read fixtures dir {}: {error}", dir.display()))
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        entries.sort();
        assert!(
            entries.len() >= 2,
            "expected at least 2 ledger fixtures, found {}",
            entries.len()
        );

        for path in entries {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let raw = fs::read_to_string(&path).unwrap();
            let ledger: Ledger = serde_json::from_str(&raw)
                .unwrap_or_else(|error| panic!("deserialize {name}: {error}"));
            let result = ledger.validate();
            assert!(
                result.is_ok(),
                "valid fixture {name} failed: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn ready_machine_rejects_human_row() {
        let ledger = Ledger {
            schema: HUMAN_ACTION_LEDGER_SCHEMA.into(),
            product: "exampleplugin".into(),
            trace_kind: TraceKind::ReadyMachine,
            primary_operation: "primary_action".into(),
            host_os: HostOs::Macos,
            rows: vec![sample_row(
                "01",
                Actor::Human,
                Kind::Human(HumanKind::GiveFact),
            )],
        };
        let codes = violation_codes(ledger.validate());
        assert!(codes.contains(&"ready-machine-human-row".to_string()));
    }

    #[test]
    fn ready_machine_rejects_say_to_user() {
        let mut row = sample_row("01", Actor::Machine, Kind::Machine(MachineKind::Validation));
        row.say_to_user_present = true;
        let ledger = Ledger {
            schema: HUMAN_ACTION_LEDGER_SCHEMA.into(),
            product: "exampleplugin".into(),
            trace_kind: TraceKind::ReadyMachine,
            primary_operation: "primary_action".into(),
            host_os: HostOs::Macos,
            rows: vec![row],
        };
        let codes = violation_codes(ledger.validate());
        assert!(codes.contains(&"ready-machine-say-to-user".to_string()));
    }

    #[test]
    fn unready_machine_rejects_invalid_human_kind() {
        let ledger = Ledger {
            schema: HUMAN_ACTION_LEDGER_SCHEMA.into(),
            product: "exampleplugin".into(),
            trace_kind: TraceKind::UnreadyMachine,
            primary_operation: "primary_action".into(),
            host_os: HostOs::Windows,
            rows: vec![sample_row(
                "01",
                Actor::Human,
                Kind::Machine(MachineKind::HostDiscovery),
            )],
        };
        let codes = violation_codes(ledger.validate());
        assert!(
            codes.contains(&"actor-kind-mismatch".to_string())
                || codes.contains(&"unready-machine-invalid-human-kind".to_string())
        );
    }

    #[test]
    fn consent_requires_material_class() {
        let ledger = Ledger {
            schema: HUMAN_ACTION_LEDGER_SCHEMA.into(),
            product: "exampleplugin".into(),
            trace_kind: TraceKind::UnreadyMachine,
            primary_operation: "primary_action".into(),
            host_os: HostOs::Windows,
            rows: vec![Row {
                step_id: "01".into(),
                operation: "primary_action".into(),
                actor: Actor::Human,
                kind: Kind::Human(HumanKind::Consent),
                envelope_status: EnvelopeStatus::NeedsHuman,
                say_to_user_present: true,
                material_class: None,
                note: None,
            }],
        };
        let codes = violation_codes(ledger.validate());
        assert!(codes.contains(&"consent-missing-material-class".to_string()));
    }

    #[test]
    fn deny_unknown_top_level_fields() {
        let raw = r#"{
            "schema": "jz.plugin.human_action_ledger.v1",
            "product": "exampleplugin",
            "trace_kind": "ready_machine",
            "primary_operation": "primary_action",
            "host_os": "macos",
            "rows": [],
            "extra": {}
        }"#;
        assert!(serde_json::from_str::<Ledger>(raw).is_err());
    }

    #[test]
    fn unready_fixture_has_exactly_one_consent_human_row() {
        let raw = fs::read_to_string(fixtures_dir().join("unready-machine.json")).unwrap();
        let ledger: Ledger = serde_json::from_str(&raw).unwrap();
        let consent_rows: Vec<_> = ledger
            .rows
            .iter()
            .filter(|row| {
                matches!(
                    (&row.actor, &row.kind),
                    (Actor::Human, Kind::Human(HumanKind::Consent))
                )
            })
            .collect();
        assert_eq!(consent_rows.len(), 1);
        assert!(consent_rows[0].material_class.is_some());
    }

    #[test]
    fn ready_fixture_has_zero_human_rows() {
        let raw = fs::read_to_string(fixtures_dir().join("ready-machine.json")).unwrap();
        let ledger: Ledger = serde_json::from_str(&raw).unwrap();
        assert!(ledger.rows.iter().all(|row| row.actor == Actor::Machine));
        assert!(!ledger.rows.iter().any(|row| row.say_to_user_present));
    }
}
