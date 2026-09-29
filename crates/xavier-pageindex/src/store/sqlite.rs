//! SQLite store implementation.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::schema::{DDL_V1, SCHEMA_VERSION};
use super::{PutOutcome, Store};
use crate::error::PageIndexError;
use crate::model::{DocStatus, Document, DocumentTree, Page, PageUnit, SourceKind, TreeNode};

fn db_err(e: rusqlite::Error) -> PageIndexError {
    PageIndexError::Store(e.to_string())
}

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PageIndexError> {
        Self::init(Connection::open(path).map_err(db_err)?)
    }

    pub fn open_in_memory() -> Result<Self, PageIndexError> {
        Self::init(Connection::open_in_memory().map_err(db_err)?)
    }

    fn init(conn: Connection) -> Result<Self, PageIndexError> {
        // journal_mode returns a row, so read it instead of executing.
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .map_err(db_err)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(db_err)?;
        conn.execute_batch(DDL_V1).map_err(db_err)?;
        let has: Option<i64> = conn
            .query_row("SELECT version FROM pi_schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_err)?;
        match has {
            None => {
                conn.execute(
                    "INSERT INTO pi_schema_version (version) VALUES (?1)",
                    [SCHEMA_VERSION],
                )
                .map_err(db_err)?;
            }
            Some(v) if v > SCHEMA_VERSION => {
                return Err(PageIndexError::Store(format!(
                    "schema version {v} is newer than supported {SCHEMA_VERSION}"
                )));
            }
            Some(_) => {}
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, PageIndexError> {
        self.conn
            .lock()
            .map_err(|_| PageIndexError::Store("connection mutex poisoned".into()))
    }

    /// Stored schema version.
    pub fn schema_version(&self) -> Result<i64, PageIndexError> {
        self.lock()?
            .query_row("SELECT version FROM pi_schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .map_err(db_err)
    }

    /// Read back a PRAGMA value (e.g. `journal_mode`, `foreign_keys`).
    pub fn pragma(&self, name: &str) -> Result<String, PageIndexError> {
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(PageIndexError::Store(format!(
                "invalid pragma name: {name}"
            )));
        }
        self.lock()?
            .query_row(&format!("PRAGMA {name}"), [], |r| {
                r.get::<_, rusqlite::types::Value>(0).map(|v| match v {
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    rusqlite::types::Value::Text(s) => s,
                    other => format!("{other:?}"),
                })
            })
            .map_err(db_err)
    }
}

fn kind_to_str(k: SourceKind) -> &'static str {
    match k {
        SourceKind::Markdown => "markdown",
        SourceKind::PlainText => "plain_text",
        SourceKind::Legal => "legal",
        SourceKind::Pdf => "pdf",
    }
}

fn kind_from_str(s: &str) -> Result<SourceKind, PageIndexError> {
    Ok(match s {
        "markdown" => SourceKind::Markdown,
        "plain_text" => SourceKind::PlainText,
        "legal" => SourceKind::Legal,
        "pdf" => SourceKind::Pdf,
        other => {
            return Err(PageIndexError::Store(format!(
                "unknown source_kind: {other}"
            )))
        }
    })
}

fn unit_to_str(u: PageUnit) -> &'static str {
    match u {
        PageUnit::Page => "page",
        PageUnit::VirtualPage => "virtual_page",
    }
}

fn unit_from_str(s: &str) -> Result<PageUnit, PageIndexError> {
    Ok(match s {
        "page" => PageUnit::Page,
        "virtual_page" => PageUnit::VirtualPage,
        other => return Err(PageIndexError::Store(format!("unknown page_unit: {other}"))),
    })
}

fn status_to_str(s: &DocStatus) -> String {
    match s {
        DocStatus::Processing => "processing".into(),
        DocStatus::Completed => "completed".into(),
        DocStatus::Failed(m) => format!("failed:{m}"),
    }
}

fn status_from_str(s: &str) -> Result<DocStatus, PageIndexError> {
    match s {
        "processing" => Ok(DocStatus::Processing),
        "completed" => Ok(DocStatus::Completed),
        _ => s
            .strip_prefix("failed:")
            .map(|m| DocStatus::Failed(m.to_string()))
            .ok_or_else(|| PageIndexError::Store(format!("unknown status: {s}"))),
    }
}

const DOC_COLS: &str = "doc_id, workspace, name, source_kind, content_hash, page_count, \
                        page_unit, status, builder, created_at";

fn row_to_doc(r: &Row<'_>) -> rusqlite::Result<Result<Document, PageIndexError>> {
    let kind: String = r.get(3)?;
    let unit: String = r.get(6)?;
    let status: String = r.get(7)?;
    let build = || -> Result<Document, PageIndexError> {
        Ok(Document {
            doc_id: r.get(0).map_err(db_err)?,
            workspace: r.get(1).map_err(db_err)?,
            name: r.get(2).map_err(db_err)?,
            source_kind: kind_from_str(&kind)?,
            content_hash: r.get(4).map_err(db_err)?,
            page_count: r.get(5).map_err(db_err)?,
            page_unit: unit_from_str(&unit)?,
            status: status_from_str(&status)?,
            builder: r.get(8).map_err(db_err)?,
            created_at: r.get(9).map_err(db_err)?,
        })
    };
    Ok(build())
}

fn query_docs(
    conn: &Connection,
    sql: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Document>, PageIndexError> {
    let mut stmt = conn.prepare(sql).map_err(db_err)?;
    let rows = stmt.query_map(args, row_to_doc).map_err(db_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(db_err)??);
    }
    Ok(out)
}

fn insert_nodes(
    tx: &rusqlite::Transaction<'_>,
    doc_id: &str,
    parent: Option<&str>,
    nodes: &[TreeNode],
) -> Result<(), PageIndexError> {
    for (ord, n) in nodes.iter().enumerate() {
        tx.execute(
            "INSERT INTO pi_nodes (doc_id, node_id, parent_id, ord, title, start_page, \
             end_page, summary, token_estimate) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                doc_id,
                n.node_id,
                parent,
                ord as i64,
                n.title,
                n.start_page,
                n.end_page,
                n.summary,
                n.token_estimate
            ],
        )
        .map_err(db_err)?;
        insert_nodes(tx, doc_id, Some(&n.node_id), &n.children)?;
    }
    Ok(())
}

impl Store for SqliteStore {
    fn put_document(
        &self,
        doc: &Document,
        tree: &DocumentTree,
        pages: &[Page],
    ) -> Result<PutOutcome, PageIndexError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(db_err)?;
        let existing: Option<(String, String, String)> = tx
            .query_row(
                "SELECT doc_id, content_hash, status FROM pi_documents \
                 WHERE workspace = ?1 AND name = ?2",
                params![doc.workspace, doc.name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db_err)?;
        if let Some((old_id, old_hash, old_status)) = existing {
            if old_hash == doc.content_hash && old_status == "completed" {
                return Ok(PutOutcome {
                    doc_id: old_id,
                    reused: true,
                });
            }
            tx.execute("DELETE FROM pi_documents WHERE doc_id = ?1", [&old_id])
                .map_err(db_err)?;
        }
        tx.execute(
            &format!(
                "INSERT INTO pi_documents ({DOC_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
            ),
            params![
                doc.doc_id,
                doc.workspace,
                doc.name,
                kind_to_str(doc.source_kind),
                doc.content_hash,
                doc.page_count,
                unit_to_str(doc.page_unit),
                status_to_str(&doc.status),
                doc.builder,
                doc.created_at
            ],
        )
        .map_err(db_err)?;
        insert_nodes(&tx, &doc.doc_id, None, &tree.roots)?;
        for p in pages {
            tx.execute(
                "INSERT INTO pi_pages (doc_id, page_no, text) VALUES (?1,?2,?3)",
                params![doc.doc_id, p.page_no, p.text],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(PutOutcome {
            doc_id: doc.doc_id.clone(),
            reused: false,
        })
    }

    fn get_document(
        &self,
        workspace: &str,
        doc_id: &str,
    ) -> Result<Option<Document>, PageIndexError> {
        let conn = self.lock()?;
        let sql =
            format!("SELECT {DOC_COLS} FROM pi_documents WHERE workspace = ?1 AND doc_id = ?2");
        Ok(query_docs(&conn, &sql, &[&workspace, &doc_id])?.pop())
    }

    fn find_by_name(
        &self,
        workspace: &str,
        name: &str,
    ) -> Result<Option<Document>, PageIndexError> {
        let conn = self.lock()?;
        let sql = format!("SELECT {DOC_COLS} FROM pi_documents WHERE workspace = ?1 AND name = ?2");
        Ok(query_docs(&conn, &sql, &[&workspace, &name])?.pop())
    }

    fn get_tree(
        &self,
        workspace: &str,
        doc_id: &str,
    ) -> Result<Option<DocumentTree>, PageIndexError> {
        if self.get_document(workspace, doc_id)?.is_none() {
            return Ok(None);
        }
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT node_id, parent_id, title, start_page, end_page, summary, \
                 token_estimate FROM pi_nodes WHERE doc_id = ?1 ORDER BY rowid",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([doc_id], |r| {
                Ok((
                    r.get::<_, Option<String>>(1)?,
                    TreeNode {
                        node_id: r.get(0)?,
                        title: r.get(2)?,
                        start_page: r.get(3)?,
                        end_page: r.get(4)?,
                        summary: r.get(5)?,
                        token_estimate: r.get(6)?,
                        children: Vec::new(),
                    },
                ))
            })
            .map_err(db_err)?;
        // Rows were inserted in preorder, so rowid order keeps a parent ahead
        // of its children and siblings in their stored order.
        let mut flat: Vec<(Option<String>, TreeNode)> = Vec::new();
        for r in rows {
            flat.push(r.map_err(db_err)?);
        }
        let index: HashMap<String, usize> = flat
            .iter()
            .enumerate()
            .map(|(i, (_, n))| (n.node_id.clone(), i))
            .collect();
        // Attach back to front: children are pushed in reverse and flipped
        // once the node is complete.
        let mut roots = Vec::new();
        let mut slots: Vec<Option<(Option<String>, TreeNode)>> =
            flat.into_iter().map(Some).collect();
        for i in (0..slots.len()).rev() {
            let (parent, mut node) = slots[i].take().expect("slot taken once");
            node.children.reverse();
            match parent {
                None => roots.push(node),
                Some(pid) => {
                    let idx = match index.get(&pid) {
                        Some(&idx) if idx < i => idx,
                        _ => {
                            return Err(PageIndexError::Store(format!(
                                "orphan node {}",
                                node.node_id
                            )))
                        }
                    };
                    slots[idx]
                        .as_mut()
                        .expect("parent not yet taken")
                        .1
                        .children
                        .push(node);
                }
            }
        }
        roots.reverse();
        Ok(Some(DocumentTree {
            doc_id: doc_id.to_string(),
            roots,
        }))
    }

    fn get_pages(
        &self,
        workspace: &str,
        doc_id: &str,
        start: u32,
        end: u32,
    ) -> Result<Vec<Page>, PageIndexError> {
        if start < 1 || start > end {
            return Err(PageIndexError::InvalidRange(format!("{start}-{end}")));
        }
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT p.page_no, p.text FROM pi_pages p \
                 JOIN pi_documents d ON d.doc_id = p.doc_id \
                 WHERE d.workspace = ?1 AND p.doc_id = ?2 AND p.page_no BETWEEN ?3 AND ?4 \
                 ORDER BY p.page_no",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![workspace, doc_id, start, end], |r| {
                Ok(Page {
                    doc_id: doc_id.to_string(),
                    page_no: r.get(0)?,
                    text: r.get(1)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    fn list_documents(&self, workspace: &str) -> Result<Vec<Document>, PageIndexError> {
        let conn = self.lock()?;
        let sql = format!(
            "SELECT {DOC_COLS} FROM pi_documents WHERE workspace = ?1 ORDER BY created_at, name"
        );
        query_docs(&conn, &sql, &[&workspace])
    }

    fn search_by_title(
        &self,
        workspace: &str,
        query: &str,
    ) -> Result<Vec<Document>, PageIndexError> {
        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let conn = self.lock()?;
        let sql = format!(
            "SELECT {DOC_COLS} FROM pi_documents d WHERE d.workspace = ?1 AND \
             (d.name LIKE ?2 ESCAPE '\\' OR EXISTS (SELECT 1 FROM pi_nodes n \
             WHERE n.doc_id = d.doc_id AND n.title LIKE ?2 ESCAPE '\\')) \
             ORDER BY d.created_at, d.name"
        );
        query_docs(&conn, &sql, &[&workspace, &pattern])
    }

    fn delete_document(&self, workspace: &str, doc_id: &str) -> Result<bool, PageIndexError> {
        let conn = self.lock()?;
        let n = conn
            .execute(
                "DELETE FROM pi_documents WHERE workspace = ?1 AND doc_id = ?2",
                params![workspace, doc_id],
            )
            .map_err(db_err)?;
        Ok(n > 0)
    }
}
