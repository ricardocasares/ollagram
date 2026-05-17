use crate::telegram::ChatId;
use aisdk::core::{Message, Messages};
use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{Arc, RwLock},
};

pub trait MessageStorage: Clone + Send + Sync + 'static {
    fn messages(&self, chat_id: ChatId) -> Result<Messages, StorageError>;
    fn replace_messages(&self, chat_id: ChatId, messages: Messages) -> Result<(), StorageError>;
    fn append_message(&self, chat_id: ChatId, message: Message) -> Result<Messages, StorageError>;
    fn clear_messages(&self, chat_id: ChatId) -> Result<(), StorageError>;
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryStorage {
    conversations: Arc<RwLock<HashMap<ChatId, Messages>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    LockPoisoned,
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LockPoisoned => write!(formatter, "storage lock poisoned"),
        }
    }
}

impl Error for StorageError {}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MessageStorage for InMemoryStorage {
    fn messages(&self, chat_id: ChatId) -> Result<Messages, StorageError> {
        self.conversations
            .read()
            .map_err(|_error| StorageError::LockPoisoned)
            .map(|conversations| conversations.get(&chat_id).cloned().unwrap_or_default())
    }

    fn replace_messages(&self, chat_id: ChatId, messages: Messages) -> Result<(), StorageError> {
        self.conversations
            .write()
            .map_err(|_error| StorageError::LockPoisoned)
            .map(|mut conversations| {
                conversations.insert(chat_id, messages);
            })
    }

    fn append_message(&self, chat_id: ChatId, message: Message) -> Result<Messages, StorageError> {
        self.conversations
            .write()
            .map_err(|_error| StorageError::LockPoisoned)
            .map(|mut conversations| {
                let messages = conversations.entry(chat_id).or_default();
                messages.push(message);
                messages.clone()
            })
    }

    fn clear_messages(&self, chat_id: ChatId) -> Result<(), StorageError> {
        self.conversations
            .write()
            .map_err(|_error| StorageError::LockPoisoned)
            .map(|mut conversations| {
                conversations.remove(&chat_id);
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aisdk::core::UserMessage;

    #[test]
    fn stores_messages_per_chat() -> Result<(), StorageError> {
        let storage = InMemoryStorage::new();
        let first_chat = 1;
        let second_chat = 2;

        storage.append_message(
            first_chat,
            Message::User(UserMessage::new("first chat message")),
        )?;
        storage.append_message(
            second_chat,
            Message::User(UserMessage::new("second chat message")),
        )?;

        assert_eq!(storage.messages(first_chat)?.len(), 1);
        assert_eq!(storage.messages(second_chat)?.len(), 1);

        Ok(())
    }

    #[test]
    fn replaces_and_clears_messages() -> Result<(), StorageError> {
        let storage = InMemoryStorage::new();
        let chat_id = 1;
        let messages = vec![Message::User(UserMessage::new("hello"))];

        storage.replace_messages(chat_id, messages)?;
        assert_eq!(storage.messages(chat_id)?.len(), 1);

        storage.clear_messages(chat_id)?;
        assert_eq!(storage.messages(chat_id)?.len(), 0);

        Ok(())
    }
}
