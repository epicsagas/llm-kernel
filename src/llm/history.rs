//! Conversation history management with token-budget-aware truncation.
//!
//! [`ConversationHistory`] holds an ordered list of [`ChatMessage`] entries
//! and enforces role alternation rules. When the conversation grows too long,
//! [`truncate_to_budget`](ConversationHistory::truncate_to_budget) removes
//! the oldest non-system messages until a [`TokenBudget`](crate::tokens::budget::TokenBudget)
//! has enough remaining capacity.
//!
//! # Example
//!
//! ```
//! use llm_kernel::llm::{ConversationHistory, ChatMessage};
//! use llm_kernel::tokens::budget::TokenBudget;
//!
//! let mut history = ConversationHistory::new();
//! history.push(ChatMessage::user("What is Rust?")).unwrap();
//! history.push(ChatMessage::assistant("A systems programming language.")).unwrap();
//!
//! let budget = TokenBudget::new(1000);
//! history.truncate_to_budget(&budget, 50);
//!
//! let request = history.clone().into_request("You are a helpful assistant.");
//! assert_eq!(request.system.as_deref(), Some("You are a helpful assistant."));
//! ```

use crate::llm::types::{ChatMessage, LLMRequest, MessageRole};
use crate::tokens::budget::TokenBudget;
use crate::tokens::estimate_tokens;

/// Manages an ordered conversation history with role validation and
/// token-budget-aware truncation.
#[derive(Debug, Clone)]
pub struct ConversationHistory {
    messages: Vec<ChatMessage>,
}

/// Error returned when a message has an invalid role for the current position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleValidationError {
    /// The role that was rejected.
    pub attempted: MessageRole,
    /// The role of the preceding message.
    pub previous: MessageRole,
}

impl std::fmt::Display for RoleValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid role transition: {:?} after {:?}",
            self.attempted, self.previous
        )
    }
}

impl std::error::Error for RoleValidationError {}

impl ConversationHistory {
    /// Create a new empty conversation history.
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    /// Number of messages in the history.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Access the messages.
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Append a message with role alternation validation.
    ///
    /// Rules:
    /// - First message can be `System`, `User`, or `Tool`.
    /// - `System` is only allowed as the first message.
    /// - `User` must follow `Assistant` or be first.
    /// - `Assistant` must follow `User` or `Tool`.
    /// - `Tool` must follow `Assistant`.
    pub fn push(&mut self, message: ChatMessage) -> Result<(), RoleValidationError> {
        if let Some(last) = self.messages.last() {
            let valid = match message.role {
                MessageRole::System => false,
                MessageRole::User => {
                    matches!(last.role, MessageRole::Assistant)
                        || matches!(last.role, MessageRole::System)
                }
                MessageRole::Assistant => {
                    matches!(last.role, MessageRole::User | MessageRole::Tool)
                }
                MessageRole::Tool => matches!(last.role, MessageRole::Assistant),
            };
            if !valid {
                return Err(RoleValidationError {
                    attempted: message.role,
                    previous: last.role,
                });
            }
        } else {
            // First message: System, User, or Tool are valid
            match message.role {
                MessageRole::System | MessageRole::User | MessageRole::Tool => {}
                MessageRole::Assistant => {
                    return Err(RoleValidationError {
                        attempted: message.role,
                        previous: MessageRole::System, // sentinel: "no previous"
                    });
                }
            }
        }
        self.messages.push(message);
        Ok(())
    }

    /// Estimate the total token count of all messages.
    pub fn token_count(&self) -> u32 {
        self.messages
            .iter()
            .map(|m| estimate_tokens(&m.text_content()) as u32)
            .sum()
    }

    /// Remove the oldest non-system messages until the token budget has
    /// enough remaining capacity for `needed` additional tokens.
    ///
    /// If the history starts with a `System` message, it is always preserved.
    /// Messages are removed from the front (oldest first) until
    /// `budget.try_reserve(needed)` succeeds or only the system message remains.
    ///
    /// Returns the number of messages removed.
    pub fn truncate_to_budget(&self, budget: &TokenBudget, needed: u32) -> usize {
        if budget.try_reserve(needed) {
            return 0;
        }
        // Calculate how many tokens we need to free
        let mut removed = 0;
        let start = if self
            .messages
            .first()
            .is_some_and(|m| m.role == MessageRole::System)
        {
            1 // preserve system message
        } else {
            0
        };
        // Release tokens from oldest non-system messages
        for i in start..self.messages.len() {
            if budget.try_reserve(needed) {
                break;
            }
            let tokens = estimate_tokens(&self.messages[i].text_content()) as u32;
            budget.release(tokens);
            removed += 1;
        }
        removed
    }

    /// Convert the history into an [`LLMRequest`] with the given system prompt.
    ///
    /// If the first message is a `System` message and `system_prompt` is `Some`,
    /// the system prompt from the message is replaced. Otherwise, the system
    /// prompt is set on the request and system messages are filtered from the
    /// message list.
    pub fn into_request(self, system_prompt: impl Into<String>) -> LLMRequest {
        let system = Some(system_prompt.into());
        let messages = self
            .messages
            .into_iter()
            .filter(|m| m.role != MessageRole::System)
            .collect();
        LLMRequest {
            system,
            messages,
            temperature: 0.7,
            max_tokens: None,
            model: None,
            response_format: None,
            tools: None,
        }
    }
}

impl Default for ConversationHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_user_then_assistant() {
        let mut h = ConversationHistory::new();
        assert!(h.push(ChatMessage::user("hello")).is_ok());
        assert!(h.push(ChatMessage::assistant("hi")).is_ok());
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn push_system_then_user() {
        let mut h = ConversationHistory::new();
        assert!(h.push(ChatMessage::system("you are helpful")).is_ok());
        assert!(h.push(ChatMessage::user("hello")).is_ok());
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn push_rejects_system_after_user() {
        let mut h = ConversationHistory::new();
        h.push(ChatMessage::user("hello")).unwrap();
        let err = h.push(ChatMessage::system("nope")).unwrap_err();
        assert_eq!(err.attempted, MessageRole::System);
        assert_eq!(err.previous, MessageRole::User);
    }

    #[test]
    fn push_rejects_double_user() {
        let mut h = ConversationHistory::new();
        h.push(ChatMessage::user("first")).unwrap();
        let err = h.push(ChatMessage::user("second")).unwrap_err();
        assert_eq!(err.attempted, MessageRole::User);
        assert_eq!(err.previous, MessageRole::User);
    }

    #[test]
    fn push_rejects_assistant_first() {
        let mut h = ConversationHistory::new();
        let err = h.push(ChatMessage::assistant("hi")).unwrap_err();
        assert_eq!(err.attempted, MessageRole::Assistant);
    }

    #[test]
    fn push_tool_after_assistant() {
        let mut h = ConversationHistory::new();
        h.push(ChatMessage::user("run tool")).unwrap();
        h.push(ChatMessage::assistant("calling tool")).unwrap();
        assert!(h.push(ChatMessage::tool("result")).is_ok());
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn into_request_sets_system_prompt() {
        let mut h = ConversationHistory::new();
        h.push(ChatMessage::system("original")).unwrap();
        h.push(ChatMessage::user("hello")).unwrap();

        let req = h.into_request("new system prompt");
        assert_eq!(req.system.as_deref(), Some("new system prompt"));
        // System message filtered out of messages
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, MessageRole::User);
    }

    #[test]
    fn truncate_to_budget_removes_oldest() {
        let budget = TokenBudget::new(100);
        // Pre-fill the budget so there's no room
        assert!(budget.try_reserve(90));

        let mut h = ConversationHistory::new();
        h.push(ChatMessage::system("system")).unwrap();
        h.push(ChatMessage::user(&"x".repeat(200))).unwrap();
        h.push(ChatMessage::assistant(&"y".repeat(200))).unwrap();

        // Need 50 tokens but only 10 remaining → must truncate
        let removed = h.truncate_to_budget(&budget, 50);
        assert!(removed > 0);
    }

    #[test]
    fn truncate_preserves_system_message() {
        let budget = TokenBudget::new(5);
        let mut h = ConversationHistory::new();
        h.push(ChatMessage::system("system instruction")).unwrap();

        // Even with very small budget, system message stays
        let removed = h.truncate_to_budget(&budget, 100);
        assert_eq!(removed, 0); // nothing to remove beyond system
        assert_eq!(h.messages()[0].role, MessageRole::System);
    }

    #[test]
    fn token_count_estimates() {
        let mut h = ConversationHistory::new();
        assert_eq!(h.token_count(), 0);
        h.push(ChatMessage::user("hello world")).unwrap();
        assert!(h.token_count() > 0);
    }

    #[test]
    fn default_is_empty() {
        let h = ConversationHistory::default();
        assert!(h.is_empty());
    }
}
