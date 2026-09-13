//! Article and clause aware legal document chunker for RAG systems.
//!
//! Provides structural parsing of legal contracts, statutes, and regulatory documents,
//! preserving hierarchical breadcrumbs ("Capítulo II > Artículo 15 > Parágrafo 1") and
//! guaranteeing that legal clauses are never truncated silently.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Represents a structured chunk extracted from a legal document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegalChunk {
    /// Unique chunk identifier.
    pub chunk_id: String,
    /// Associated document identifier.
    pub document_id: String,
    /// Optional numeric clause index (e.g., 15 for Artículo 15 or 1 for Cláusula Primera).
    pub clause_number: Option<u32>,
    /// Structural title of the clause or section.
    pub clause_title: String,
    /// Full breadcrumb path indicating structural hierarchy.
    pub full_path: String,
    /// Text content of the chunk.
    pub text: String,
    /// Estimated token count.
    pub token_count: usize,
    /// Additional metadata key-value attributes.
    pub metadata: HashMap<String, String>,
}

/// Hierarchical legal document chunker engine.
#[derive(Debug, Clone)]
pub struct LegalHierarchicalChunker {
    /// Maximum target token count per chunk.
    pub max_tokens: usize,
    /// Token overlap window for sub-chunking long clauses.
    pub overlap_tokens: usize,
}

impl Default for LegalHierarchicalChunker {
    fn default() -> Self {
        Self {
            max_tokens: 500,
            overlap_tokens: 50,
        }
    }
}

/// Structural heading classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderKind {
    Chapter,
    Article,
    Clause,
    Paragraph,
    Preamble,
}

struct ParsedHeader {
    kind: HeaderKind,
    raw_title: String,
    clause_number: Option<u32>,
}

static CHAPTER_REGEX: OnceLock<Regex> = OnceLock::new();
static ARTICLE_REGEX: OnceLock<Regex> = OnceLock::new();
static CLAUSE_REGEX: OnceLock<Regex> = OnceLock::new();
static PARAGRAPH_REGEX: OnceLock<Regex> = OnceLock::new();
static PREAMBLE_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_chapter_regex() -> &'static Regex {
    CHAPTER_REGEX.get_or_init(|| {
        Regex::new(r"(?i)^\s*(?:C\s*A\s*P\s*[ÍI]\s*T\s*U\s*L\s*O|T\s*[ÍI]\s*T\s*U\s*L\s*O|C\s*H\s*A\s*P\s*T\s*E\s*R|T\s*I\s*T\s*L\s*E)\s*([IVXLCDM0-9\s]+|[\wáéíóúÁÉÍÓÚ\s]+)?").unwrap()
    })
}

fn get_article_regex() -> &'static Regex {
    ARTICLE_REGEX.get_or_init(|| {
        Regex::new(r"(?i)^\s*(?:A\s*R\s*T\s*[ÍI]\s*C\s*U\s*L\s*O|A\s*R\s*T\s*\.|A\s*R\s*T\s*I\s*C\s*L\s*E)\s*([0-9\s]+|[IVXLCDM]+|[\wáéíóúÁÉÍÓÚ\s]+)?").unwrap()
    })
}

fn get_clause_regex() -> &'static Regex {
    CLAUSE_REGEX.get_or_init(|| {
        Regex::new(r"(?i)^\s*(?:C\s*L\s*[ÁA]\s*U\s*S\s*U\s*L\s*A|C\s*L\s*A\s*U\s*S\s*E)\s*([0-9\s]+|[IVXLCDM]+|PRIMERA|SEGUNDA|TERCERA|CUARTA|QUINTA|SEXTA|SÉPTIMA|SEPTIMA|OCTAVA|NOVENA|DÉCIMA|DECIMA|FIRST|SECOND|THIRD|FOURTH|FIFTH|[\wáéíóúÁÉÍÓÚ\s]+)?").unwrap()
    })
}

fn get_paragraph_regex() -> &'static Regex {
    PARAGRAPH_REGEX.get_or_init(|| {
        Regex::new(r"(?i)^\s*(?:P\s*A\s*R\s*[ÁA]\s*G\s*R\s*A\s*F\s*O|P\s*A\s*R\s*A\s*G\s*R\s*A\s*P\s*H|S\s*U\s*B\s*-\s*C\s*L\s*[ÁA]\s*U\s*S\s*U\s*L\s*A|S\s*U\s*B\s*C\s*L\s*A\s*U\s*S\s*U\s*L\s*A)\s*([0-9\s]+|[IVXLCDM]+|[ÚU]\s*N\s*I\s*C\s*O|ONE|TWO|[\wáéíóúÁÉÍÓÚ\s]+)?").unwrap()
    })
}

fn get_preamble_regex() -> &'static Regex {
    PREAMBLE_REGEX.get_or_init(|| {
        Regex::new(r"(?i)^\s*(?:C\s*O\s*N\s*S\s*I\s*D\s*E\s*R\s*A\s*N\s*D\s*O|A\s*N\s*T\s*E\s*C\s*E\s*D\s*E\s*N\s*T\s*E\s*S|P\s*R\s*E\s*A\s*M\s*B\s*L\s*E|W\s*H\s*E\s*R\s*E\s*A\s*S)").unwrap()
    })
}

/// Fast token count approximation (~1.3 tokens per word).
pub fn estimate_token_count(text: &str) -> usize {
    let word_count = text.split_whitespace().count();
    if word_count == 0 {
        0
    } else {
        (word_count as f64 * 1.3).ceil() as usize
    }
}

/// Normalizes spacing from OCR scanned text (e.g. "A R T Í C U L O" -> "ARTÍCULO").
pub fn clean_whitespace(input: &str) -> String {
    let collapsed = collapse_ocr_spaces(input);
    collapsed
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

fn collapse_ocr_spaces(input: &str) -> String {
    let words: Vec<&str> = input.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }
    let mut result = String::new();
    let mut i = 0;
    while i < words.len() {
        if words[i].chars().count() == 1 {
            let is_alpha = words[i].chars().next().is_some_and(|c| c.is_alphabetic());
            let is_digit = words[i].chars().next().is_some_and(|c| c.is_ascii_digit());

            if is_alpha || is_digit {
                let mut j = i;
                while j < words.len() && words[j].chars().count() == 1 {
                    let next_alpha = words[j].chars().next().is_some_and(|c| c.is_alphabetic());
                    let next_digit = words[j].chars().next().is_some_and(|c| c.is_ascii_digit());

                    if (is_alpha && next_alpha) || (is_digit && next_digit) {
                        result.push_str(words[j]);
                        j += 1;
                    } else {
                        break;
                    }
                }
                result.push(' ');
                i = j;
                continue;
            }
        }
        result.push_str(words[i]);
        result.push(' ');
        i += 1;
    }
    result.trim().to_string()
}

fn parse_number_from_text(s: &str) -> Option<u32> {
    let mut digit_buf = String::new();
    let mut found_digit = false;

    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digit_buf.push(ch);
            found_digit = true;
        } else if found_digit {
            if ch.is_whitespace() {
                continue;
            } else {
                break;
            }
        }
    }

    if !digit_buf.is_empty() {
        if let Ok(num) = digit_buf.parse::<u32>() {
            return Some(num);
        }
    }

    let upper = s.to_uppercase();
    let words: Vec<&str> = upper.split_whitespace().collect();
    for word in words {
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
        match clean {
            "PRIMERA" | "PRIMERO" | "FIRST" | "I" => return Some(1),
            "SEGUNDA" | "SEGUNDO" | "SECOND" | "II" => return Some(2),
            "TERCERA" | "TERCERO" | "THIRD" | "III" => return Some(3),
            "CUARTA" | "CUARTO" | "FOURTH" | "IV" => return Some(4),
            "QUINTA" | "QUINTO" | "FIFTH" | "V" => return Some(5),
            "SEXTA" | "SEXTO" | "SIXTH" | "VI" => return Some(6),
            "SÉPTIMA" | "SEPTIMA" | "SEPTIMO" | "SEVENTH" | "VII" => return Some(7),
            "OCTAVA" | "OCTAVO" | "EIGHTH" | "VIII" => return Some(8),
            "NOVENA" | "NOVENO" | "NINTH" | "IX" => return Some(9),
            "DÉCIMA" | "DECIMA" | "DECIMO" | "TENTH" | "X" => return Some(10),
            _ => {}
        }
    }

    None
}

fn detect_header(line: &str) -> Option<ParsedHeader> {
    let normalized = clean_whitespace(line);

    if get_preamble_regex().is_match(&normalized) || get_preamble_regex().is_match(line) {
        return Some(ParsedHeader {
            kind: HeaderKind::Preamble,
            raw_title: normalized,
            clause_number: None,
        });
    }

    if get_chapter_regex().is_match(&normalized) || get_chapter_regex().is_match(line) {
        let num = parse_number_from_text(&normalized);
        return Some(ParsedHeader {
            kind: HeaderKind::Chapter,
            raw_title: normalized,
            clause_number: num,
        });
    }

    if get_article_regex().is_match(&normalized) || get_article_regex().is_match(line) {
        let num = parse_number_from_text(&normalized);
        return Some(ParsedHeader {
            kind: HeaderKind::Article,
            raw_title: normalized,
            clause_number: num,
        });
    }

    if get_clause_regex().is_match(&normalized) || get_clause_regex().is_match(line) {
        let num = parse_number_from_text(&normalized);
        return Some(ParsedHeader {
            kind: HeaderKind::Clause,
            raw_title: normalized,
            clause_number: num,
        });
    }

    if get_paragraph_regex().is_match(&normalized) || get_paragraph_regex().is_match(line) {
        let num = parse_number_from_text(&normalized);
        return Some(ParsedHeader {
            kind: HeaderKind::Paragraph,
            raw_title: normalized,
            clause_number: num,
        });
    }

    None
}

#[derive(Debug, Clone)]
struct SectionAccumulator {
    chapter: Option<String>,
    clause: Option<String>,
    paragraph: Option<String>,
    clause_number: Option<u32>,
    paragraphs: Vec<String>,
}

impl SectionAccumulator {
    fn new() -> Self {
        Self {
            chapter: None,
            clause: None,
            paragraph: None,
            clause_number: None,
            paragraphs: Vec::new(),
        }
    }

    fn full_path(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ref ch) = self.chapter {
            parts.push(ch.as_str());
        }
        if let Some(ref cl) = self.clause {
            parts.push(cl.as_str());
        }
        if let Some(ref pa) = self.paragraph {
            parts.push(pa.as_str());
        }

        if parts.is_empty() {
            "Preamble".to_string()
        } else {
            parts.join(" > ")
        }
    }

    fn clause_title(&self) -> String {
        if let Some(ref pa) = self.paragraph {
            pa.clone()
        } else if let Some(ref cl) = self.clause {
            cl.clone()
        } else if let Some(ref ch) = self.chapter {
            ch.clone()
        } else {
            "Preamble".to_string()
        }
    }

    fn is_empty(&self) -> bool {
        self.paragraphs.is_empty()
    }
}

impl LegalHierarchicalChunker {
    /// Creates a new `LegalHierarchicalChunker` with specified token parameters.
    pub fn new(max_tokens: usize, overlap_tokens: usize) -> Self {
        Self {
            max_tokens: max_tokens.max(50),
            overlap_tokens: overlap_tokens.min(max_tokens / 2),
        }
    }

    /// Chunks a legal document into structured, breadcrumb-aware `LegalChunk` objects.
    pub fn chunk_document(&self, document_id: &str, raw_text: &str) -> Vec<LegalChunk> {
        let lines: Vec<&str> = raw_text.lines().collect();
        let mut chunks = Vec::new();
        let mut section = SectionAccumulator::new();
        let mut chunk_counter = 1;

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(header) = detect_header(trimmed) {
                // Flush previous section if it contains content
                if !section.is_empty() {
                    self.flush_section(document_id, &section, &mut chunks, &mut chunk_counter);
                    section.paragraphs.clear();
                }

                match header.kind {
                    HeaderKind::Preamble => {
                        section.chapter = Some(header.raw_title);
                        section.clause = None;
                        section.paragraph = None;
                        section.clause_number = None;
                    }
                    HeaderKind::Chapter => {
                        section.chapter = Some(header.raw_title);
                        section.clause = None;
                        section.paragraph = None;
                        section.clause_number = header.clause_number;
                    }
                    HeaderKind::Article | HeaderKind::Clause => {
                        section.clause = Some(header.raw_title);
                        section.paragraph = None;
                        section.clause_number = header.clause_number;
                    }
                    HeaderKind::Paragraph => {
                        section.paragraph = Some(header.raw_title);
                        if header.clause_number.is_some() {
                            section.clause_number = header.clause_number;
                        }
                    }
                }
            } else {
                section.paragraphs.push(clean_whitespace(trimmed));
            }
        }

        if !section.is_empty() {
            self.flush_section(document_id, &section, &mut chunks, &mut chunk_counter);
        }

        chunks
    }

    fn flush_section(
        &self,
        document_id: &str,
        section: &SectionAccumulator,
        out_chunks: &mut Vec<LegalChunk>,
        chunk_counter: &mut usize,
    ) {
        let full_text = section.paragraphs.join("\n\n");
        let token_count = estimate_token_count(&full_text);
        let full_path = section.full_path();
        let title = section.clause_title();

        if token_count <= self.max_tokens {
            let mut metadata = HashMap::new();
            metadata.insert("full_path".to_string(), full_path.clone());
            metadata.insert("clause_title".to_string(), title.clone());
            if let Some(num) = section.clause_number {
                metadata.insert("clause_number".to_string(), num.to_string());
            }
            metadata.insert("part_index".to_string(), "1".to_string());
            metadata.insert("total_parts".to_string(), "1".to_string());

            let chunk_id = format!("{}_{}", document_id, chunk_counter);
            *chunk_counter += 1;

            out_chunks.push(LegalChunk {
                chunk_id,
                document_id: document_id.to_string(),
                clause_number: section.clause_number,
                clause_title: title,
                full_path,
                text: full_text,
                token_count,
                metadata,
            });
        } else {
            // Split into sub-chunks with overlap and part indexing
            let sub_chunks = self.split_into_windowed_subchunks(&section.paragraphs);
            let total_parts = sub_chunks.len();

            for (idx, sub_text) in sub_chunks.into_iter().enumerate() {
                let part_num = idx + 1;
                let sub_title = format!("{} (Part {}/{})", title, part_num, total_parts);
                let sub_path = format!("{} (Part {}/{})", full_path, part_num, total_parts);
                let sub_tokens = estimate_token_count(&sub_text);

                let mut metadata = HashMap::new();
                metadata.insert("full_path".to_string(), sub_path.clone());
                metadata.insert("clause_title".to_string(), sub_title.clone());
                if let Some(num) = section.clause_number {
                    metadata.insert("clause_number".to_string(), num.to_string());
                }
                metadata.insert("part_index".to_string(), part_num.to_string());
                metadata.insert("total_parts".to_string(), total_parts.to_string());
                metadata.insert("is_subclause".to_string(), "true".to_string());

                let chunk_id = format!("{}_{}_{}", document_id, chunk_counter, part_num);

                out_chunks.push(LegalChunk {
                    chunk_id,
                    document_id: document_id.to_string(),
                    clause_number: section.clause_number,
                    clause_title: sub_title,
                    full_path: sub_path,
                    text: sub_text,
                    token_count: sub_tokens,
                    metadata,
                });
            }
            *chunk_counter += 1;
        }
    }

    fn split_into_windowed_subchunks(&self, paragraphs: &[String]) -> Vec<String> {
        let mut results = Vec::new();
        let mut current_window: Vec<String> = Vec::new();
        let mut current_tokens = 0;

        for para in paragraphs {
            let para_tokens = estimate_token_count(para);

            if current_tokens + para_tokens > self.max_tokens && !current_window.is_empty() {
                results.push(current_window.join("\n\n"));

                // Retain overlap paragraphs from the tail of current window
                let mut overlap_window: Vec<String> = Vec::new();
                let mut overlap_count = 0;
                for prev_para in current_window.iter().rev() {
                    let p_tok = estimate_token_count(prev_para);
                    if overlap_count + p_tok <= self.overlap_tokens {
                        overlap_window.insert(0, prev_para.clone());
                        overlap_count += p_tok;
                    } else {
                        break;
                    }
                }

                current_window = overlap_window;
                current_tokens = overlap_count;
            }

            current_window.push(para.clone());
            current_tokens += para_tokens;
        }

        if !current_window.is_empty() {
            results.push(current_window.join("\n\n"));
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colombian_contract_parsing() {
        let text = r#"
CONSIDERANDO
Las partes acuerdan suscribir el presente contrato de prestación de servicios.

CAPÍTULO I
DISPOSICIONES GENERALES

CLÁUSULA PRIMERA. OBJETO
El contratista se obliga a prestar los servicios de consultoría técnica en inteligencia artificial.

ARTÍCULO 12. OBLIGACIONES DEL CONTRATANTE
El contratante pagará la suma pactada en las fechas establecidas.

PARÁGRAFO 1. PAGO ANTICIPADO
Cualquier pago anticipado requerirá la aprobación previa de la junta directiva.
"#;

        let chunker = LegalHierarchicalChunker::new(500, 50);
        let chunks = chunker.chunk_document("doc_co_01", text);

        assert!(!chunks.is_empty());
        let first_clause = chunks.iter().find(|c| c.clause_number == Some(1)).unwrap();
        assert_eq!(first_clause.clause_number, Some(1));
        assert!(first_clause.full_path.contains("CAPÍTULO I"));

        let art12 = chunks.iter().find(|c| c.clause_number == Some(12)).unwrap();
        assert_eq!(art12.clause_number, Some(12));
        assert!(art12.full_path.contains("ARTÍCULO 12"));

        let parag1 = chunks
            .iter()
            .find(|c| c.full_path.contains("PARÁGRAFO 1"))
            .unwrap();
        assert_eq!(parag1.clause_number, Some(1));
        assert_eq!(
            parag1.full_path,
            "CAPÍTULO I > ARTÍCULO 12. OBLIGACIONES DEL CONTRATANTE > PARÁGRAFO 1. PAGO ANTICIPADO"
        );
    }

    #[test]
    fn test_english_contract_parsing() {
        let text = r#"
PREAMBLE
This Agreement is entered into on this day between Party A and Party B.

CHAPTER 1
SERVICES

ARTICLE 5. SCOPE OF WORK
The Contractor shall provide software engineering services as defined in Exhibit A.

CLAUSE 3. TERMINATION
Either party may terminate this agreement upon 30 days written notice.
"#;

        let chunker = LegalHierarchicalChunker::new(500, 50);
        let chunks = chunker.chunk_document("doc_en_01", text);

        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].full_path, "PREAMBLE");

        let art5 = chunks.iter().find(|c| c.clause_number == Some(5)).unwrap();
        assert_eq!(art5.clause_number, Some(5));
        assert_eq!(art5.full_path, "CHAPTER 1 > ARTICLE 5. SCOPE OF WORK");

        let clause3 = chunks.iter().find(|c| c.clause_number == Some(3)).unwrap();
        assert_eq!(clause3.clause_number, Some(3));
        assert_eq!(clause3.full_path, "CHAPTER 1 > CLAUSE 3. TERMINATION");
    }

    #[test]
    fn test_ocr_spacing_tolerance() {
        let text = r#"
C A P Í T U L O  I I
A R T Í C U L O  1 5
El licenciatario defenderá e indemnizará al licenciante contra cualquier reclamación de terceros.
P A R Á G R A F O  1
La indemnización no excederá el valor total del contrato.
"#;

        let chunker = LegalHierarchicalChunker::new(500, 50);
        let chunks = chunker.chunk_document("doc_ocr_01", text);

        assert!(!chunks.is_empty());
        let art15 = chunks.iter().find(|c| c.clause_number == Some(15)).unwrap();
        assert_eq!(art15.clause_number, Some(15));
        assert!(art15.full_path.contains("ARTÍCULO"));

        let parag1 = chunks
            .iter()
            .find(|c| c.full_path.contains("PARÁGRAFO"))
            .unwrap();
        assert_eq!(parag1.clause_number, Some(1));
    }

    #[test]
    fn test_oversized_clause_splitting_with_overlap() {
        let mut paragraphs = Vec::new();
        for i in 1..=20 {
            paragraphs.push(format!(
                "Párrafo número {} con suficiente texto explicativo detalldado sobre términos y condiciones jurídicas de responsabilidad legal.",
                i
            ));
        }

        let mut text = String::from("CLÁUSULA TERCERA. RESPONSABILIDAD DE LAS PARTES\n");
        text.push_str(&paragraphs.join("\n\n"));

        let chunker = LegalHierarchicalChunker::new(100, 20);
        let chunks = chunker.chunk_document("doc_large_01", &text);

        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert_eq!(chunk.clause_number, Some(3));
            assert!(chunk.metadata.contains_key("is_subclause"));
            assert!(chunk.clause_title.contains("Part"));
        }
    }

    #[test]
    fn test_breadcrumb_hierarchy_building() {
        let text = r#"
CAPÍTULO I. DE LAS DEFINICIONES
CLÁUSULA SEGUNDA. DEFINICIONES
Para todos los efectos de este contrato, los siguientes términos tendrán el significado que aquí se les asigna.

PARÁGRAFO ÚNICO
Definición de fuerza mayor o caso fortuito.
"#;

        let chunker = LegalHierarchicalChunker::new(500, 50);
        let chunks = chunker.chunk_document("doc_hierarchy_01", text);

        assert!(!chunks.is_empty());
        let parag = chunks
            .iter()
            .find(|c| c.full_path.contains("PARÁGRAFO ÚNICO"))
            .unwrap();
        assert_eq!(
            parag.full_path,
            "CAPÍTULO I. DE LAS DEFINICIONES > CLÁUSULA SEGUNDA. DEFINICIONES > PARÁGRAFO ÚNICO"
        );
        assert_eq!(parag.clause_number, Some(2));
    }
}
