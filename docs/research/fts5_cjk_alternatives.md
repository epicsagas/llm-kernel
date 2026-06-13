# Pre-Research on FTS5 CJK Tokenizer Lightweighting & Independent Index Design

This document reviews the feasibility of **SQLite FTS5 C-API integration** versus **application-side independent inverted index design** for supporting CJK (Chinese, Japanese, Korean) full-text search (FTS) in the `llm-kernel` project, prior to entering the `v0.7.0` (Transport & Backend) roadmap phase.

---

## 1. Background & Problem Definition

Currently, `llm-kernel` utilizes SQLite's built-in `trigram` tokenizer in [schema.rs](file:///Users/hackme/projects/epiccounty/llm-kernel/src/graph/schema.rs#L49-L51):
```sql
CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts
    USING fts5(title, body, tags, content=nodes, content_rowid=rowid, tokenize='trigram');
```

### Limitations of the `trigram` Tokenizer
1. **Search Noise (False Positives)**: Splitting text purely into 3-character blocks (trigrams) causes a significant amount of noise and false positives when searching CJK characters.
2. **Portability Issues**: Some mobile, cloud, or legacy SQLite binaries are compiled without the `trigram` extension or FTS5 entirely, resulting in compilation or runtime errors.
3. **Lack of DBMS Portability**: Since `trigram` is SQLite-specific, it creates maintenance fragmentation when implementing the PostgreSQL backend planned for `v0.8.0`, as PostgreSQL uses a completely different FTS syntax.

---

## 2. Option A: Pure Rust SQLite FTS5 Custom Tokenizer (FFI)

This approach binds SQLite FTS5's C-API through Rust's `extern "C"` block, registering a lightweight CJK N-gram tokenizer implemented in Rust directly into SQLite.

### 2.1 Implementation Structure (C FFI Integration)
To register a custom tokenizer with SQLite FTS5, C-API structs and callbacks must be bound and provided:

```rust
use std::os::raw::{c_char, c_int, c_void};

#[repr(C)]
pub struct Fts5Tokenizer;

#[repr(C)]
pub struct fts5_api {
    iVersion: c_int,
    xCreateTokenizer: unsafe extern "C" fn(
        pApi: *mut fts5_api,
        zName: *const c_char,
        pContext: *mut c_void,
        pTokenizer: *mut fts5_tokenizer,
        xDestroy: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> c_int,
    // ... other FTS5 API function pointers
}

// 1. xCreate: Allocates and initializes the tokenizer instance
unsafe extern "C" fn xCreate(
    pUnused: *mut c_void,
    azArg: *mut *const c_char,
    nArg: c_int,
    ppOut: *mut *mut Fts5Tokenizer,
) -> c_int {
    0 // SQLITE_OK
}

// 2. xTokenize: Segments text and invokes the token callback
unsafe extern "C" fn xTokenize(
    pTokenizer: *mut Fts5Tokenizer,
    pCtx: *mut c_void,
    flags: c_int,
    pText: *const c_char,
    nText: c_int,
    xToken: Option<unsafe extern "C" fn(
        pCtx: *mut c_void,
        tflags: c_int,
        pToken: *const c_char,
        nToken: c_int,
        iStart: c_int,
        iEnd: c_int,
    ) -> c_int>,
) -> c_int {
    0 // SQLITE_OK
}
```

### 2.2 Pros & Cons
* 👍 **Pros**:
  * Leverages existing FTS5 virtual table schemas and SQL `MATCH` syntax without modifications.
  * Utilizes SQLite's built-in triggers, ensuring indexes are automatically updated atomically on write operations.
* 👎 **Cons**:
  * **Highly Complex C FFI**: Requires raw pointers and `unsafe` blocks, increasing the risk of memory leaks and segmentation faults.
  * **Build Complexity**: Requires tight coupling with the `libsqlite3-sys` FFI, which can fail to build on target platforms with restrictive compiler environments (e.g., missing `cc` compiler).

---

## 3. Option B: Application-side Inverted Index Design (SQLite-independent)

This approach completely bypasses SQLite's FTS5 extension, designing a standard relational table as the inverted index storage. Tokenization and query parsing are handled entirely in safe, pure Rust code.

### 3.1 Inverted Index Data Model (Relational Schema)
Instead of virtual tables, standard tables and indexes manage the tokens:

```sql
-- Stores unique terms extracted from node texts along with their frequencies
CREATE TABLE IF NOT EXISTS node_postings (
    term         TEXT NOT NULL,
    node_id      TEXT NOT NULL,
    frequency    INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (term, node_id),
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
);

-- Index for maximizing query performance
CREATE INDEX IF NOT EXISTS idx_postings_term ON node_postings(term);
```

### 3.2 Pure Rust CJK N-gram Tokenizer Implementation
Implemented in pure Rust (`src/tokens/tokenizer.rs`) without external C dependencies:

```rust
fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{3000}'..='\u{303F}' | // CJK symbols and punctuation
        '\u{3040}'..='\u{309F}' | // Hiragana
        '\u{30A0}'..='\u{30FF}' | // Katakana
        '\u{3130}'..='\u{318F}' | // Hangul Compatibility Jamo
        '\u{AC00}'..='\u{D7AF}' | // Hangul Syllables
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
    )
}

pub fn tokenize_cjk_ngram(text: &str, n: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    
    while i < chars.len() {
        if is_cjk(chars[i]) {
            if i + n <= chars.len() && chars[i..i+n].iter().all(|&c| is_cjk(c)) {
                let token: String = chars[i..i+n].iter().collect();
                tokens.push(token);
            } else {
                tokens.push(chars[i].to_string());
            }
            i += 1;
        } else {
            let mut word = String::new();
            while i < chars.len() && !is_cjk(chars[i]) && !chars[i].is_whitespace() && !chars[i].is_ascii_punctuation() {
                word.push(chars[i]);
                i += 1;
            }
            if !word.is_empty() {
                tokens.push(word.to_lowercase());
            }
            i += 1;
        }
    }
    tokens
}
```

### 3.3 Index Updates & Search Flow
* **On Insert/Update**:
  ```rust
  // 1. Delete existing tokens
  conn.execute("DELETE FROM node_postings WHERE node_id = ?", params![node.id])?;
  
  // 2. Tokenize new text and calculate frequencies
  let text_to_index = format!("{} {} {}", node.title, node.body, node.tags.join(" "));
  let tokens = tokenize_cjk_ngram(&text_to_index, 2);
  let mut counts = HashMap::new();
  for token in tokens {
      *counts.entry(token).or_insert(0) += 1;
  }
  
  // 3. Bulk insert
  let mut stmt = conn.prepare("INSERT INTO node_postings (term, node_id, frequency) VALUES (?, ?, ?)")?;
  for (term, freq) in counts {
      stmt.execute(params![term, node.id, freq])?;
  }
  ```
* **On Search (SQL Matching & Importance Weighting)**:
  ```sql
  SELECT n.id, n.title, n.importance, SUM(p.frequency) as match_score
  FROM nodes n
  JOIN node_postings p ON n.id = p.node_id
  WHERE p.term IN (?1, ?2, ?3)
  GROUP BY n.id
  ORDER BY (match_score * 0.4 + n.importance * 0.6) DESC
  LIMIT ?4;
  ```

---

## 4. Technical Comparison Matrix

| Comparison Metric | Option A: FTS5 Custom Tokenizer (FFI) | Option B: Application-side Inverted Index (Rust) |
| :--- | :---: | :---: |
| **Code Safety** | 🔴 Low (unsafe block, raw pointer manipulation) | 🟢 Highest (Safe Rust only) |
| **Compilation Deps** | 🟡 Medium (enforces libsqlite3-sys linking) | 🟢 None (zero external C compiler dependency) |
| **DBMS Portability** | 🔴 Low (bound to SQLite FTS5 C-API) | 🟢 Highest (run on PostgreSQL, SQLite, etc. instantly) |
| **Search Customizability** | 🟡 Medium (requires complex C/FFI logic) | 🟢 High (utilize Rust's ecosystem effortlessly) |

---

## 5. Conclusion & Action Plan

`llm-kernel`'s core design philosophy values **"zero-mandatory-external-deps"** and **"maximum portability and flexibility"**. Keeping C ABI bindings (Option A) poses a risk of breaking cross-platform compilation, compromising library flexibility.

Therefore, we recommend adopting **[Option B: Application-side Inverted Index Design]**. When starting `v0.7.0`, we should refactor the graph schema and node store to use the relational `node_postings` index model.
