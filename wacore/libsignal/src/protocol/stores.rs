// Re-exporting structures from waproto to avoid duplication
#[cfg(test)]
pub use waproto::whatsapp::SenderKeyRecordStructure;
pub use waproto::whatsapp::{
    IdentityKeyPairStructure, PreKeyRecordStructure, SenderKeyStateStructure, SessionStructure,
    SignedPreKeyRecordStructure,
};

pub use waproto::whatsapp::sender_key_state_structure;
pub use waproto::whatsapp::session_structure;
