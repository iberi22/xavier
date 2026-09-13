use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use regex::Regex;

/// Represents a single chunk extracted from a legal document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegalChunk {
    pub chunk_id: String,
    pub document_id: String,
    pub clause_number: Option<u32>,
    pub clause_title: String,
    pub full_path: String,
    pub text: String,
    pub token_count: usize,
    pub metadata: HashMap<String, String>,
}

/// Hierarchical chunking engine for legal documents (contracts, statutes, etc.).
pub struct LegalHierarchicalChunker {
    max_tokens: usize,
    overlap_tokens: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum LegalSectionType {
    Preamble,
    Considerando,
    Capitulo,
    Seccion,
    Articulo,
    Clausula,
    Paragrafo,
    Other,
}

#[derive(Debug, Clone)]
struct LegalSectionHeader {
    section_type: LegalSectionType,
    number: Option<u32>,
    title: String,
    raw_header: String,
    line_index: usize,
}

impl Default for LegalHierarchicalChunker {
    fn default() -> Self {
        Self::new(500, 50)
    }
}

impl LegalHierarchicalChunker {
    /// Creates a new LegalHierarchicalChunker instance.
    pub fn new(max_tokens: usize, overlap_tokens: usize) -> Self {
        Self {
            max_tokens: if max_tokens == 0 { 500 } else { max_tokens },
            overlap_tokens,
        }
    }

    /// Fast token count approximation (~1.3 tokens per word).
    pub fn estimate_tokens(text: &str) -> usize {
        let words = text.split_whitespace().count();
        if words == 0 {
            return 0;
        }
        (words * 13 + 9) / 10
    }

    /// Cleans irregular OCR whitespace (e.g., "A R T Í C U L O" -> "ARTÍCULO").
    pub fn clean_whitespace(text: &str) -> String {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut result_lines = Vec::new();

        for line in normalized.lines() {
            let mut line_str = line.trim().to_string();
            let re_ocr = Regex::new(r"(?i)\b([A-ZÁÉÍÓÚÑ])(\s+[A-ZÁÉÍÓÚÑ])+\b").unwrap();
            line_str = re_ocr.replace_all(&line_str, |caps: &regex::Captures| {
                let matched = caps.get(0).unwrap().as_str();
                let chars: Vec<&str> = matched.split_whitespace().collect();
                if chars.iter().all(|c| c.chars().count() == 1) {
                    chars.join("")
                } else {
                    matched.to_string()
                }
            }).to_string();

            let words: Vec<&str> = line_str.split_whitespace().collect();
            result_lines.push(words.join(" "));
        }

        result_lines.join("\n")
    }

    /// Chunks a legal document into structured, clause-aware chunks preserving breadcrumb hierarchy.
    pub fn chunk_document(&self, document_id: &str, raw_text: &str) -> Vec<LegalChunk> {
        let text = Self::clean_whitespace(raw_text);
        let lines: Vec<&str> = text.lines().collect();

        if lines.is_empty() || text.trim().is_empty() {
            return Vec::new();
        }

        // Patterns matching standard legal headers (Spanish & English)
        let re_capitulo = Regex::new(r"(?i)^(CAPÍTULO|CAPITULO|CHAPTER)\s+([IVXLCDM\d]+|\b[A-ZÁÉÍÓÚ]+\b)(?::?\s*(.*))?$").unwrap();
        let re_seccion = Regex::new(r"(?i)^(SECCIÓN|SECCION|SECTION)\s+([IVXLCDM\d]+|\b[A-ZÁÉÍÓÚ]+\b)(?::?\s*(.*))?$").unwrap();
        let re_articulo = Regex::new(r"(?i)^(ARTÍCULO|ARTICULO|ARTICLE)\s+(\d+|[IVXLCDM]+)(?::?\s*(.*))?$").unwrap();
        let re_clausula = Regex::new(r"(?i)^(CLÁUSULA|CLAUSULA|CLAUSE)\s+([A-ZÁÉÍÓÚ\d]+|PRIMERA|SEGUNDA|TERCERA|CUARTA|QUINTA|SEXTA|SÉPTIMA|OCTAVA|NOVENA|DÉCIMA|DÉCIMAPRIMERA|FIRST|SECOND|THIRD|FOURTH|FIFTH)(?::?\s*(.*))?$").unwrap();
        let re_paragrafo = Regex::new(r"(?i)^(PARÁGRAFO|PARAGRAFO|PARAGRAPH)\s*(\d+|[IVXLCDM]+|ÚNICO|UNICO)?(?::?\s*(.*))?$").unwrap();
        let re_considerando = Regex::new(r"(?i)^(CONSIDERANDO|WHEREAS)(?::?\s*(.*))?$").unwrap();
        let re_preambulo = Regex::new(r"(?i)^(PREÁMBULO|PREAMBULO|PREAMBLE)(?::?\s*(.*))?$").unwrap();

        let mut headers: Vec<LegalSectionHeader> = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            let line_trimmed = line.trim();
            if line_trimmed.is_empty() {
                continue;
            }

            if let Some(caps) = re_capitulo.captures(line_trimmed) {
                let kw = caps.get(1).map_or("Capítulo", |m| m.as_str());
                let num_str = caps.get(2).map_or("", |m| m.as_str());
                let title = caps.get(3).map_or("", |m| m.as_str()).trim();
                headers.push(LegalSectionHeader {
                    section_type: LegalSectionType::Capitulo,
                    number: Self::parse_number(num_str),
                    title: if title.is_empty() { format!("{} {}", kw, num_str) } else { format!("{} {}: {}", kw, num_str, title) },
                    raw_header: line_trimmed.to_string(),
                    line_index: idx,
                });
            } else if let Some(caps) = re_seccion.captures(line_trimmed) {
                let kw = caps.get(1).map_or("Sección", |m| m.as_str());
                let num_str = caps.get(2).map_or("", |m| m.as_str());
                let title = caps.get(3).map_or("", |m| m.as_str()).trim();
                headers.push(LegalSectionHeader {
                    section_type: LegalSectionType::Seccion,
                    number: Self::parse_number(num_str),
                    title: if title.is_empty() { format!("{} {}", kw, num_str) } else { format!("{} {}: {}", kw, num_str, title) },
                    raw_header: line_trimmed.to_string(),
                    line_index: idx,
                });
            } else if let Some(caps) = re_articulo.captures(line_trimmed) {
                let kw = caps.get(1).map_or("Artículo", |m| m.as_str());
                let num_str = caps.get(2).map_or("", |m| m.as_str());
                let title = caps.get(3).map_or("", |m| m.as_str()).trim();
                headers.push(LegalSectionHeader {
                    section_type: LegalSectionType::Articulo,
                    number: Self::parse_number(num_str),
                    title: if title.is_empty() { format!("{} {}", kw, num_str) } else { format!("{} {}: {}", kw, num_str, title) },
                    raw_header: line_trimmed.to_string(),
                    line_index: idx,
                });
            } else if let Some(caps) = re_clausula.captures(line_trimmed) {
                let kw = caps.get(1).map_or("Cláusula", |m| m.as_str());
                let num_str = caps.get(2).map_or("", |m| m.as_str());
                let title = caps.get(3).map_or("", |m| m.as_str()).trim();
                headers.push(LegalSectionHeader {
                    section_type: LegalSectionType::Clausula,
                    number: Self::parse_number(num_str),
                    title: if title.is_empty() { format!("{} {}", kw, num_str) } else { format!("{} {}: {}", kw, num_str, title) },
                    raw_header: line_trimmed.to_string(),
                    line_index: idx,
                });
            } else if let Some(caps) = re_paragrafo.captures(line_trimmed) {
                let kw = caps.get(1).map_or("Parágrafo", |m| m.as_str());
                let num_str = caps.get(2).map_or("", |m| m.as_str());
                let title = caps.get(3).map_or("", |m| m.as_str()).trim();
                headers.push(LegalSectionHeader {
                    section_type: LegalSectionType::Paragrafo,
                    number: Self::parse_number(num_str),
                    title: if title.is_empty() {
                        if num_str.is_empty() { kw.to_string() } else { format!("{} {}", kw, num_str) }
                    } else {
                        format!("{} {}: {}", kw, num_str, title)
                    },
                    raw_header: line_trimmed.to_string(),
                    line_index: idx,
                });
            } else if let Some(caps) = re_considerando.captures(line_trimmed) {
                let kw = caps.get(1).map_or("Considerando", |m| m.as_str());
                let title = caps.get(2).map_or("", |m| m.as_str()).trim();
                headers.push(LegalSectionHeader {
                    section_type: LegalSectionType::Considerando,
                    number: None,
                    title: if title.is_empty() { kw.to_string() } else { format!("{}: {}", kw, title) },
                    raw_header: line_trimmed.to_string(),
                    line_index: idx,
                });
            } else if let Some(caps) = re_preambulo.captures(line_trimmed) {
                let kw = caps.get(1).map_or("Preámbulo", |m| m.as_str());
                let title = caps.get(2).map_or("", |m| m.as_str()).trim();
                headers.push(LegalSectionHeader {
                    section_type: LegalSectionType::Preamble,
                    number: None,
                    title: if title.is_empty() { kw.to_string() } else { format!("{}: {}", kw, title) },
                    raw_header: line_trimmed.to_string(),
                    line_index: idx,
                });
            }
        }

        if headers.is_empty() {
            return self.split_large_text(
                document_id,
                None,
                "Document Content",
                "Document Content",
                &text,
                0,
            );
        }

        let mut chunks = Vec::new();
        let mut active_capitulo: Option<String> = None;
        let mut active_seccion: Option<String> = None;
        let mut active_main: Option<(String, Option<u32>)> = None;

        if headers[0].line_index > 0 {
            let preamble_lines = &lines[0..headers[0].line_index];
            let preamble_text = preamble_lines.join("\n").trim().to_string();
            if !preamble_text.is_empty() {
                chunks.extend(self.split_large_text(
                    document_id,
                    None,
                    "Preámbulo",
                    "Preámbulo",
                    &preamble_text,
                    chunks.len(),
                ));
            }
        }

        for (h_idx, header) in headers.iter().enumerate() {
            let start_line = header.line_index;
            let end_line = if h_idx + 1 < headers.len() {
                headers[h_idx + 1].line_index
            } else {
                lines.len()
            };

            let section_lines = &lines[start_line..end_line];
            let section_text = section_lines.join("\n").trim().to_string();

            match header.section_type {
                LegalSectionType::Capitulo => {
                    active_capitulo = Some(header.title.clone());
                    active_seccion = None;
                    active_main = None;
                }
                LegalSectionType::Seccion => {
                    active_seccion = Some(header.title.clone());
                    active_main = None;
                }
                LegalSectionType::Articulo | LegalSectionType::Clausula => {
                    active_main = Some((header.title.clone(), header.number));
                }
                _ => {}
            }

            let mut path_parts = Vec::new();
            if let Some(ref cap) = active_capitulo {
                path_parts.push(cap.clone());
            }
            if let Some(ref sec) = active_seccion {
                path_parts.push(sec.clone());
            }

            match header.section_type {
                LegalSectionType::Capitulo => {
                    if path_parts.is_empty() {
                        path_parts.push(header.title.clone());
                    }
                }
                LegalSectionType::Seccion => {
                    if active_seccion.is_none() || path_parts.last() != Some(&header.title) {
                        path_parts.push(header.title.clone());
                    }
                }
                LegalSectionType::Articulo | LegalSectionType::Clausula => {
                    path_parts.push(header.title.clone());
                }
                LegalSectionType::Paragrafo | LegalSectionType::Considerando | LegalSectionType::Preamble => {
                    if let Some((ref main_title, _)) = active_main {
                        path_parts.push(main_title.clone());
                    }
                    path_parts.push(header.title.clone());
                }
                LegalSectionType::Other => {
                    path_parts.push(header.title.clone());
                }
            }

            let full_path = path_parts.join(" > ");
            let clause_num = header.number.or_else(|| active_main.as_ref().and_then(|(_, num)| *num));

            chunks.extend(self.split_large_text(
                document_id,
                clause_num,
                &header.title,
                &full_path,
                &section_text,
                chunks.len(),
            ));
        }

        chunks
    }

    fn parse_number(num_str: &str) -> Option<u32> {
        let cleaned = num_str.trim().trim_matches('.');
        if let Ok(n) = cleaned.parse::<u32>() {
            return Some(n);
        }

        match cleaned.to_uppercase().as_str() {
            "I" | "PRIMERA" | "FIRST" | "1" => Some(1),
            "II" | "SEGUNDA" | "SECOND" | "2" => Some(2),
            "III" | "TERCERA" | "THIRD" | "3" => Some(3),
            "IV" | "CUARTA" | "FOURTH" | "4" => Some(4),
            "V" | "QUINTA" | "FIFTH" | "5" => Some(5),
            "VI" | "SEXTA" | "6" => Some(6),
            "VII" | "SÉPTIMA" | "SEPTIMA" | "7" => Some(7),
            "VIII" | "OCTAVA" | "8" => Some(8),
            "IX" | "NOVENA" | "9" => Some(9),
            "X" | "DÉCIMA" | "DECIMA" | "10" => Some(10),
            _ => None,
        }
    }

    /// Splits clause text if it exceeds `max_tokens`, ensuring sub-clause parts (Part 1/N) are preserved without truncation.
    fn split_large_text(
        &self,
        document_id: &str,
        clause_number: Option<u32>,
        clause_title: &str,
        full_path: &str,
        text: &str,
        start_chunk_idx: usize,
    ) -> Vec<LegalChunk> {
        let total_tokens = Self::estimate_tokens(text);

        if total_tokens <= self.max_tokens {
            let mut metadata = HashMap::new();
            metadata.insert("hierarchy_path".to_string(), full_path.to_string());
            if let Some(num) = clause_number {
                metadata.insert("clause_number".to_string(), num.to_string());
            }

            return vec![LegalChunk {
                chunk_id: format!("{}_chunk_{}", document_id, start_chunk_idx + 1),
                document_id: document_id.to_string(),
                clause_number,
                clause_title: clause_title.to_string(),
                full_path: full_path.to_string(),
                text: text.to_string(),
                token_count: total_tokens,
                metadata,
            }];
        }

        let paragraphs: Vec<&str> = text.split("\n\n").filter(|p| !p.trim().is_empty()).collect();
        let mut sub_chunks = Vec::new();
        let mut current_paras: Vec<&str> = Vec::new();
        let mut current_tokens = 0;

        for para in paragraphs {
            let para_tokens = Self::estimate_tokens(para);
            if current_tokens + para_tokens > self.max_tokens && !current_paras.is_empty() {
                sub_chunks.push(current_paras.join("\n\n"));
                current_paras.clear();
                current_tokens = 0;
            }

            current_paras.push(para);
            current_tokens += para_tokens;
        }

        if !current_paras.is_empty() {
            sub_chunks.push(current_paras.join("\n\n"));
        }

        let total_parts = sub_chunks.len();
        let mut chunks = Vec::new();

        for (idx, sub_text) in sub_chunks.into_iter().enumerate() {
            let sub_title = format!("{} (Part {}/{})", clause_title, idx + 1, total_parts);
            let sub_tokens = Self::estimate_tokens(&sub_text);

            let mut metadata = HashMap::new();
            metadata.insert("hierarchy_path".to_string(), full_path.to_string());
            metadata.insert("part".to_string(), format!("{}/{}", idx + 1, total_parts));
            if let Some(num) = clause_number {
                metadata.insert("clause_number".to_string(), num.to_string());
            }

            chunks.push(LegalChunk {
                chunk_id: format!("{}_chunk_{}_part{}", document_id, start_chunk_idx + chunks.len() + 1, idx + 1),
                document_id: document_id.to_string(),
                clause_number,
                clause_title: sub_title,
                full_path: full_path.to_string(),
                text: sub_text,
                token_count: sub_tokens,
                metadata,
            });
        }

        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spanish_contract_chunking() {
        let chunker = LegalHierarchicalChunker::new(500, 50);
        let doc = r#"
CAPÍTULO I: DISPOSICIONES GENERALES

ARTÍCULO 1: OBJETO
El presente contrato tiene por objeto regular la prestación de servicios profesionales.

ARTÍCULO 2: PRECIO Y FORMA DE PAGO
El valor total del contrato asciende a la suma de diez millones de pesos.

PARÁGRAFO 1: Los pagos se realizarán dentro de los primeros cinco días de cada mes.

CLÁUSULA TERCERA: GARANTÍAS
El contratista otorgará una póliza de cumplimiento a favor del contratante.
"#;

        let chunks = chunker.chunk_document("doc_es_1", doc);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.clause_title.contains("ARTÍCULO 1") || c.text.contains("OBJETO")));
        assert!(chunks.iter().any(|c| c.full_path.contains("CAPÍTULO I") && c.full_path.contains("ARTÍCULO 1")));
        assert!(chunks.iter().any(|c| c.clause_title.contains("CLÁUSULA TERCERA") || c.clause_title.contains("GARANTÍAS")));
    }

    #[test]
    fn test_english_contract_chunking() {
        let chunker = LegalHierarchicalChunker::new(500, 50);
        let doc = r#"
CHAPTER 1: PRELIMINARY PROVISIONS

ARTICLE 1: DEFINITIONS
The terms used in this Agreement shall have the meanings set forth below.

ARTICLE 2: SCOPE OF SERVICES
The Provider agrees to perform the services detailed in Schedule A.

PARAGRAPH 1: All deliverables must meet strict quality assurance benchmarks.

CLAUSE 3: TERMINATION
Either party may terminate this agreement with 30 days written notice.
"#;

        let chunks = chunker.chunk_document("doc_en_1", doc);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.full_path.contains("CHAPTER 1") && c.full_path.contains("ARTICLE 1")));
        assert!(chunks.iter().any(|c| c.clause_title.contains("CLAUSE 3") || c.text.contains("TERMINATION")));
    }

    #[test]
    fn test_preamble_and_considerando_chunking() {
        let chunker = LegalHierarchicalChunker::new(500, 50);
        let doc = r#"
PREÁMBULO
Este acuerdo se celebra entre las partes identificadas a continuación.

CONSIDERANDO
Que la primera parte es una sociedad comercial legalmente constituida.
Que la segunda parte cuenta con la experiencia técnica requerida.

CLÁUSULA PRIMERA: OBJETO
Las partes acuerdan cooperar en el desarrollo del proyecto.
"#;

        let chunks = chunker.chunk_document("doc_preamble", doc);
        assert!(chunks.iter().any(|c| c.clause_title.contains("PREÁMBULO") || c.full_path.contains("PREÁMBULO") || c.clause_title.contains("Preámbulo")));
        assert!(chunks.iter().any(|c| c.clause_title.contains("CONSIDERANDO") || c.full_path.contains("CONSIDERANDO") || c.clause_title.contains("Considerando")));
        assert!(chunks.iter().any(|c| c.clause_title.contains("CLÁUSULA PRIMERA")));
    }

    #[test]
    fn test_messy_ocr_spacing_handling() {
        let chunker = LegalHierarchicalChunker::new(500, 50);
        let doc = r#"
C A P Í T U L O I
A R T Í C U L O 15: OBLIGACIONES
El contratista se compromete a cumplir con todas las especificaciones técnicas.
P A R Á G R A F O 1: En caso de mora, se aplicará una sanción diaria.
"#;

        let chunks = chunker.chunk_document("doc_ocr", doc);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.full_path.contains("CAPÍTULO") || c.full_path.contains("ARTÍCULO 15") || c.full_path.contains("Capítulo") || c.full_path.contains("Artículo 15")));
    }

    #[test]
    fn test_overlong_clause_splitting() {
        let chunker = LegalHierarchicalChunker::new(20, 5); // Small token limit to force split
        let doc = r#"
ARTÍCULO 10: OBLIGACIONES EXTENSAS

Esta es la primera parte super larga de la cláusula legal que contiene muchísimas palabras y detalles legales minuciosos para obligar a las partes a cumplir adecuadamente.

Esta es la segunda parte super larga de la misma cláusula legal que detalta las penalidades severas, multas coercitivas y términos contractuales adicionales para garantizar el cumplimiento.
"#;

        let chunks = chunker.chunk_document("doc_overlong", doc);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].clause_title.contains("Part 1/"));
        assert!(chunks[0].metadata.contains_key("part"));
        let part_val = chunks[0].metadata.get("part").unwrap();
        assert!(part_val.starts_with("1/"));
    }
}
