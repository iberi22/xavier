//! Xavier Legal & Tax Document Generator & AST Diffing Engine
//!
//! Provides deterministic document synthesis, structured AST representation,
//! fiscal/tax table computation, and incremental JSON-patch style regeneration
//! for tax & legal advisory firms (e.g. Restrepo & Londoño).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// A formal legal or tax document represented as an Abstract Syntax Tree (AST).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegalDocument {
    pub id: String,
    pub title: String,
    pub doc_type: LegalDocType,
    pub jurisdiction: String,
    pub version: u32,
    pub parties: Vec<LegalParty>,
    pub clauses: Vec<LegalClause>,
    pub fiscal_tables: Vec<FiscalTable>,
    pub metadata: HashMap<String, String>,
    pub integrity_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LegalDocType {
    TaxAdvisoryContract,
    PowerOfAttorney,
    TaxAppealDIAN,
    LegalMemorandum,
    ShareholderAgreement,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegalParty {
    pub role: String, // e.g., "ASESOR", "CLIENTE", "PODERDANTE", "APODERADO"
    pub name: String,
    pub tax_id: String, // NIT or Cedula
    pub city: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegalClause {
    pub id: String,
    pub number: u32,
    pub title: String,
    pub body: String,
    pub is_modified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FiscalTable {
    pub id: String,
    pub title: String,
    pub currency: String,
    pub items: Vec<FiscalItem>,
    pub subtotal: f64,
    pub retention_percentage: f64,
    pub retention_amount: f64,
    pub vat_percentage: f64,
    pub vat_amount: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FiscalItem {
    pub concept: String,
    pub base_amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DocPatchOp {
    UpdateClause {
        clause_id: String,
        new_body: String,
    },
    AddClause {
        number: u32,
        title: String,
        body: String,
    },
    RemoveClause {
        clause_id: String,
    },
    RecalculateFiscalTable {
        table_id: String,
        retention_pct: f64,
        vat_pct: f64,
    },
}

impl FiscalTable {
    /// Recalculates subtotals, tax withholdings and totals deterministically.
    pub fn recalculate(&mut self) {
        let subtotal: f64 = self.items.iter().map(|item| item.base_amount).sum();
        self.subtotal = subtotal;
        self.retention_amount = (subtotal * self.retention_percentage / 100.0).round();
        self.vat_amount = (subtotal * self.vat_percentage / 100.0).round();
        self.total = subtotal + self.vat_amount - self.retention_amount;
    }
}

impl LegalDocument {
    /// Creates a new LegalDocument instance.
    pub fn new(
        id: String,
        title: String,
        doc_type: LegalDocType,
        jurisdiction: String,
        parties: Vec<LegalParty>,
    ) -> Self {
        Self {
            id,
            title,
            doc_type,
            jurisdiction,
            version: 1,
            parties,
            clauses: Vec::new(),
            fiscal_tables: Vec::new(),
            metadata: HashMap::new(),
            integrity_hash: None,
        }
    }

    /// Computes deterministic SHA-256 hash representing the document's legal seal.
    pub fn seal(&mut self) -> String {
        self.integrity_hash = None; // Reset for canonical serialization
        let canonical_json = serde_json::to_string(self).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(canonical_json.as_bytes());
        let result = hasher.finalize();
        let mut hash = String::with_capacity(64);
        for byte in result {
            use std::fmt::Write;
            let _ = write!(hash, "{:02x}", byte);
        }
        self.integrity_hash = Some(hash.clone());
        hash
    }

    /// Applies differential patches to regenerate specific clauses or recalculate tax tables.
    pub fn apply_patch(&mut self, ops: Vec<DocPatchOp>) {
        for op in ops {
            match op {
                DocPatchOp::UpdateClause {
                    clause_id,
                    new_body,
                } => {
                    if let Some(clause) = self.clauses.iter_mut().find(|c| c.id == clause_id) {
                        clause.body = new_body;
                        clause.is_modified = true;
                    }
                }
                DocPatchOp::AddClause {
                    number,
                    title,
                    body,
                } => {
                    let id = format!("clause-{}", self.clauses.len() + 1);
                    self.clauses.push(LegalClause {
                        id,
                        number,
                        title,
                        body,
                        is_modified: true,
                    });
                }
                DocPatchOp::RemoveClause { clause_id } => {
                    self.clauses.retain(|c| c.id != clause_id);
                }
                DocPatchOp::RecalculateFiscalTable {
                    table_id,
                    retention_pct,
                    vat_pct,
                } => {
                    if let Some(table) = self.fiscal_tables.iter_mut().find(|t| t.id == table_id) {
                        table.retention_percentage = retention_pct;
                        table.vat_percentage = vat_pct;
                        table.recalculate();
                    }
                }
            }
        }
        self.version += 1;
        self.seal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legal_document_lifecycle() {
        let mut doc = LegalDocument::new(
            "doc-001".to_string(),
            "Contrato de Asesoría Tributaria Integral".to_string(),
            LegalDocType::TaxAdvisoryContract,
            "CO-BOG".to_string(),
            vec![
                LegalParty {
                    role: "ASESOR".to_string(),
                    name: "Restrepo & Londoño Abogados".to_string(),
                    tax_id: "900.123.456-7".to_string(),
                    city: "Bogotá D.C.".to_string(),
                },
                LegalParty {
                    role: "CLIENTE".to_string(),
                    name: "Corporación Ejemplo S.A.S.".to_string(),
                    tax_id: "800.987.654-1".to_string(),
                    city: "Medellín".to_string(),
                },
            ],
        );

        // Add initial clause
        doc.apply_patch(vec![DocPatchOp::AddClause {
            number: 1,
            title: "OBJETO DEL CONTRATO".to_string(),
            body: "El ASESOR se compromete a prestar servicios de asesoría tributaria continua."
                .to_string(),
        }]);

        // Add fiscal table
        let mut table = FiscalTable {
            id: "table-fees".to_string(),
            title: "Liquidación de Honorarios Mensuales".to_string(),
            currency: "COP".to_string(),
            items: vec![FiscalItem {
                concept: "Honorarios Mensuales Asesoría Tributaria".to_string(),
                base_amount: 10_000_000.0,
            }],
            subtotal: 0.0,
            retention_percentage: 11.0,
            retention_amount: 0.0,
            vat_percentage: 19.0,
            vat_amount: 0.0,
            total: 0.0,
        };
        table.recalculate();
        doc.fiscal_tables.push(table);

        let initial_hash = doc.seal();
        assert_eq!(doc.version, 2);
        assert!(!initial_hash.is_empty());
        assert_eq!(doc.fiscal_tables[0].total, 10_800_000.0);

        // Apply differential regeneration (update clause and retention)
        doc.apply_patch(vec![
            DocPatchOp::UpdateClause {
                clause_id: "clause-1".to_string(),
                new_body: "El ASESOR prestará servicios de consultoría fiscal preventiva y defensa cambiaria.".to_string(),
            },
            DocPatchOp::RecalculateFiscalTable {
                table_id: "table-fees".to_string(),
                retention_pct: 4.0, // Retención servicios generales
                vat_pct: 19.0,
            },
        ]);

        assert_eq!(doc.version, 3);
        assert_ne!(doc.integrity_hash.unwrap(), initial_hash);
        assert_eq!(doc.fiscal_tables[0].retention_amount, 400_000.0);
        assert_eq!(doc.fiscal_tables[0].total, 11_500_000.0);
        assert!(doc.clauses[0].is_modified);
    }
}
