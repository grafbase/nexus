use std::collections::HashSet;

use convert_case::Boundary;
use rmcp::{
    model::Tool,
    serde_json::{self, Map, Value},
};
use tantivy::{
    Index, IndexReader, IndexWriter, TantivyDocument, Term,
    collector::TopDocs,
    doc,
    query::{BooleanQuery, BoostQuery, DisjunctionMaxQuery, FuzzyTermQuery, Occur, Query, TermQuery},
    schema::{Field, IndexRecordOption, STORED, Schema, TEXT, Value as _},
};

use crate::downstream::ToolId;

const HEAP_SIZE: usize = 50 * 1024 * 1024; // 50MB
const TOP_DOCS_LIMIT: usize = 10; // Only fetch what we need
const MAX_RESULTS: usize = 10;

/// A search index for tools that provides efficient full-text search capabilities.
///
/// The index stores tools with their metadata and provides fuzzy search functionality
/// across tool names, descriptions, and input parameters.
pub struct ToolIndex {
    reader: IndexReader,
    writer: IndexWriter,
    fields: IndexFields,
}

/// Internal field definitions for the search index.
struct IndexFields {
    /// Human-readable title for the tool
    tool_title: Field,
    /// Original tool name (without server prefix)
    tool_name: Field,
    /// Server name (without tool suffix)
    server_name: Field,
    /// Tool description
    description: Field,
    /// JSON string of input parameters
    input_params: Field,
    /// Tokenized searchable content
    search_tokens: Field,
    /// Tool ID
    id: Field,
}

/// A search result containing a tool and its relevance score.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    /// The tool that matched the search query
    pub tool_id: ToolId,
    /// The relevance score for this result (higher is more relevant)
    pub score: f32,
}

impl ToolIndex {
    /// Creates a new empty tool index.
    pub fn new() -> anyhow::Result<Self> {
        let mut builder = Schema::builder();

        let fields = IndexFields {
            tool_title: builder.add_text_field("tool_title", TEXT | STORED),
            tool_name: builder.add_text_field("tool_name", TEXT | STORED),
            server_name: builder.add_text_field("server_name", TEXT | STORED),
            description: builder.add_text_field("description", TEXT | STORED),
            input_params: builder.add_text_field("input_params", TEXT | STORED),
            search_tokens: builder.add_text_field("search_tokens", TEXT | STORED),
            id: builder.add_text_field("id", STORED),
        };

        let schema = builder.build();
        let index = Index::create_in_ram(schema);
        let reader = index.reader()?;
        let writer = index.writer(HEAP_SIZE)?;

        Ok(Self { reader, writer, fields })
    }

    /// Adds a tool to the index.
    ///
    /// The tool name must be in the format "server_name__tool_name" where "__" separates
    /// the server name from the tool name.
    pub fn add_tool(&mut self, tool: &Tool, id: ToolId) -> anyhow::Result<()> {
        let Some((server_name, tool_name)) = tool.name.split_once("__") else {
            return Err(anyhow::anyhow!("Invalid tool name format: missing server name"));
        };

        let mut doc = doc!(
            self.fields.tool_name => tool_name,
            self.fields.server_name => server_name,
        );

        if let Some(ref description) = tool.description {
            doc.add_text(self.fields.description, description);
        }

        if !tool.input_schema.is_empty() {
            let input_schema = serde_json::to_string(&tool.input_schema)?;
            doc.add_text(self.fields.input_params, &input_schema);
        }

        if let Some(ref annotations) = tool.annotations
            && let Some(ref title) = annotations.title
        {
            doc.add_text(self.fields.tool_title, title);
        }

        let search_tokens = self.generate_search_tokens(tool)?;
        doc.add_text(self.fields.search_tokens, &search_tokens);

        doc.add_u64(self.fields.id, id.into());

        self.writer.add_document(doc)?;

        Ok(())
    }

    /// Commits all pending changes to the index and reloads the reader.
    ///
    /// This must be called after adding tools to make them available for searching.
    pub fn commit(&mut self) -> anyhow::Result<()> {
        self.writer.commit()?;
        self.reader.reload()?;

        Ok(())
    }

    /// Searches for tools matching the given keywords.
    ///
    /// The search uses a combination of exact matching and fuzzy matching across
    /// different fields, with relevance scoring to rank results.
    ///
    /// # Arguments
    ///
    /// * `keywords` - An iterator of search terms
    pub fn search<'a, I>(&self, keywords: I) -> anyhow::Result<Vec<SearchResult>>
    where
        I: IntoIterator<Item = &'a str>,
        I::IntoIter: ExactSizeIterator,
    {
        let searcher = self.reader.searcher();
        let keywords = keywords.into_iter();

        if keywords.len() == 0 {
            return Ok(Vec::new());
        }

        let query = self.build_combined_search_query(keywords)?;

        // Use exact limit instead of 100
        let top_docs = searcher.search(&query, &TopDocs::with_limit(TOP_DOCS_LIMIT))?;

        let mut results = Vec::with_capacity(TOP_DOCS_LIMIT);
        let mut seen_tools = HashSet::new();

        for (score, doc_address) in top_docs {
            let doc = searcher.doc(doc_address)?;
            if let Ok(search_result) = self.doc_to_search_result(&doc, score) {
                // Deduplicate by tool name
                if seen_tools.insert(search_result.tool_id) {
                    results.push(search_result);

                    if results.len() >= MAX_RESULTS {
                        break;
                    }
                }
            }
        }

        Ok(results)
    }

    /// Converts a Tantivy document to a search result.
    fn doc_to_search_result(&self, doc: &TantivyDocument, score: f32) -> anyhow::Result<SearchResult> {
        let tool_id = doc
            .get_first(self.fields.id)
            .and_then(|v| v.as_u64())
            .ok_or(anyhow::anyhow!("field is not u64"))?
            .into();

        Ok(SearchResult { tool_id, score })
    }

    /// Generates searchable tokens from a tool's metadata.
    fn generate_search_tokens(&self, tool: &Tool) -> anyhow::Result<String> {
        let Some((server_name, tool_name)) = tool.name.split_once("__") else {
            return Err(anyhow::anyhow!("Invalid tool name format: missing server name"));
        };

        let mut buffer = String::with_capacity(256);

        for (i, token) in tokenize_name(server_name).into_iter().enumerate() {
            if i > 0 {
                buffer.push(' ');
            }
            buffer.push_str(&token);
        }

        for token in tokenize_name(tool_name) {
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(&token);
        }

        if let Some(ref desc) = tool.description {
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(desc);
        }

        for token in tokenize_map(&tool.input_schema) {
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(&token);
        }

        Ok(buffer)
    }

    /// Builds a combined search query from keywords.
    fn build_combined_search_query<'a>(
        &self,
        keywords: impl ExactSizeIterator<Item = &'a str>,
    ) -> anyhow::Result<Box<dyn Query>> {
        let mut main_queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        for keyword in keywords {
            let terms = parse_query_terms(keyword);
            let mut keyword_queries: Vec<Box<dyn Query>> = Vec::new();

            for term in terms {
                // Only use fuzzy for terms longer than 4 chars and when it makes sense
                let use_fuzzy = term.len() > 4 && !term.chars().all(|c| c.is_ascii_digit());

                let mut term_queries = Vec::new();

                // Exact matches in important fields (higher scores)
                self.add_exact_term_queries(&term, &mut term_queries);

                // Fuzzy matches only when beneficial
                if use_fuzzy {
                    self.add_fuzzy_term_queries(&term, &mut term_queries);
                }

                if !term_queries.is_empty() {
                    keyword_queries.push(Box::new(DisjunctionMaxQuery::new(term_queries)));
                }
            }

            if !keyword_queries.is_empty() {
                main_queries.push((Occur::Should, Box::new(DisjunctionMaxQuery::new(keyword_queries))));
            }
        }

        Ok(Box::new(BooleanQuery::new(main_queries)))
    }

    /// Adds exact term queries for important fields.
    fn add_exact_term_queries(&self, term: &str, queries: &mut Vec<Box<dyn Query>>) {
        // Focus on most relevant fields with appropriate scoring
        let important_fields = [
            (self.fields.tool_name, 3.0),
            (self.fields.tool_title, 2.0),
            (self.fields.description, 1.2),
            (self.fields.server_name, 0.8),
        ];

        for (field, boost) in important_fields {
            let term_obj = Term::from_field_text(field, term);
            let query = Box::new(BoostQuery::new(
                Box::new(TermQuery::new(term_obj, IndexRecordOption::Basic)),
                boost,
            ));
            queries.push(query);
        }
    }

    /// Adds fuzzy term queries for less critical fields.
    fn add_fuzzy_term_queries(&self, term: &str, queries: &mut Vec<Box<dyn Query>>) {
        // Add fuzzy queries for less critical fields
        let fuzzy_fields = [
            (self.fields.description, 0.6),
            (self.fields.input_params, 0.4),
            (self.fields.search_tokens, 0.3),
        ];

        for (field, boost) in fuzzy_fields {
            let term_obj = Term::from_field_text(field, term);
            let fuzzy_query = Box::new(FuzzyTermQuery::new(term_obj, 1, true));
            let boosted_query = Box::new(BoostQuery::new(fuzzy_query, boost));
            queries.push(boosted_query);
        }
    }
}

/// Extracts searchable tokens from a JSON object map.
///
/// Recursively traverses the map and tokenizes all string keys.
fn tokenize_map(map: &Map<String, Value>) -> Vec<String> {
    let mut tokens = Vec::new();

    for (key, value) in map.iter() {
        tokens.extend(tokenize_name(key));

        if let Value::Object(map) = value {
            tokens.extend(tokenize_map(map))
        }
    }

    tokens
}

/// Tokenizes a name string into searchable terms.
///
/// Splits the name on word boundaries and converts to lowercase,
/// filtering out single characters and empty strings.
fn tokenize_name(name: &str) -> Vec<String> {
    convert_case::split(&name, &Boundary::defaults())
        .into_iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty() && s.len() > 1) // Filter out single chars
        .collect()
}

/// Parses a query string into individual search terms.
///
/// Splits the query on whitespace and then further tokenizes each term
/// using word boundaries, filtering out short terms.
fn parse_query_terms(query: &str) -> Vec<String> {
    // Pre-allocate with estimated capacity
    let mut terms = Vec::with_capacity(query.split_whitespace().count() * 2);

    for term in query.split_whitespace() {
        let converted = convert_case::split(&term, &Boundary::defaults())
            .into_iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty() && s.len() > 1);

        terms.extend(converted);
    }

    terms
}
