use std::collections::HashMap;

/// Searches for trigger words and returns the corresponding custom response.
///
/// This is a generic version that works with any message type by using
/// an extraction function to get the content from each message.
///
/// # Arguments
/// * `messages` - The messages to search through
/// * `custom_responses` - A map of trigger words to their corresponding responses
/// * `extract_content` - A function that extracts the content string from a message
///
/// # Returns
/// * `Some(response)` - If a trigger word is found in any message
/// * `None` - If no trigger words are found
pub fn find_custom_response<T>(
    messages: &[T],
    custom_responses: &HashMap<String, String>,
    extract_content: impl Fn(&T) -> &str,
) -> Option<String> {
    for message in messages {
        let content = extract_content(message);
        for (trigger, response) in custom_responses {
            if content.contains(trigger) {
                return Some(response.clone());
            }
        }
    }
    None
}

/// Searches for trigger words in a single text string.
///
/// Simplified version for providers that work with concatenated text
/// rather than structured messages.
///
/// # Arguments
/// * `text` - The text to search through
/// * `custom_responses` - A map of trigger words to their corresponding responses
///
/// # Returns
/// * `Some(response)` - If a trigger word is found
/// * `None` - If no trigger words are found
pub fn find_custom_response_in_text(text: &str, custom_responses: &HashMap<String, String>) -> Option<String> {
    for (trigger, response) in custom_responses {
        if text.contains(trigger) {
            return Some(response.clone());
        }
    }
    None
}
