#![allow(clippy::disallowed_methods)]

use std::mem::{align_of, size_of};
use waproto::buffa::Message as _;
use waproto::buffa::{UnknownField, UnknownFieldData, UnknownFields};
use waproto::whatsapp::__unknown_storage::Storage;

fn future() -> UnknownFields {
    let mut nested = UnknownFields::new();
    nested.push(UnknownField {
        number: 202,
        data: UnknownFieldData::LengthDelimited(vec![0x55; 1024]),
    });
    let mut fields = UnknownFields::new();
    fields.push(UnknownField {
        number: 201,
        data: UnknownFieldData::Group(nested),
    });
    fields
}

#[test]
fn storage_keeps_layout_and_owned_unknown_records() {
    assert_eq!(size_of::<Storage>(), size_of::<UnknownFields>());
    assert_eq!(align_of::<Storage>(), align_of::<UnknownFields>());
    let mut storage = Storage::from(future());
    let cloned = storage.clone();
    storage.clear();
    assert!(storage.is_empty());
    assert!(storage.clone().is_empty());
    let restored = UnknownFields::from(cloned);
    assert_eq!(restored, future());
    let storage = Storage::from(restored);
    let fields: Vec<_> = storage.into_iter().collect();
    assert_eq!(fields, future().into_iter().collect::<Vec<_>>());
}

#[test]
fn empty_retained_capacity_and_replaced_owners_drop_once() {
    for _ in 0..8 {
        let mut storage = Storage::from(future());
        storage.retain(|_| false);
        assert!(storage.is_empty());
        let empty = std::mem::replace(&mut storage, Storage::from(future()));
        drop(empty);
        let owned = UnknownFields::from(std::mem::take(&mut storage));
        assert_eq!(owned, future());
        drop(storage);
        drop(owned);
    }
}

#[test]
fn shared_message_clones_keep_unknown_wire_after_original_is_dropped() {
    fn check<T: waproto::buffa::Message + Clone>() {
        // Future field 1000 holds an owned payload.
        let wire = [0xc2, 0x3e, 4, 11, 22, 33, 44];
        let message = T::decode_from_slice(&wire).unwrap();
        let cloned = message.clone();
        drop(message);
        assert_eq!(cloned.encode_to_vec(), wire);
    }
    check::<waproto::whatsapp::Message>();
    check::<waproto::whatsapp::ContextInfo>();
    check::<waproto::whatsapp::BotMetadata>();
    check::<waproto::whatsapp::MessageContextInfo>();

    let mut message = waproto::whatsapp::Message::default();
    message.conversation = Some("synthetic clone fixture".into());
    let cloned = message.clone();
    message.conversation.as_mut().unwrap().clear();
    assert_eq!(
        cloned.conversation.as_deref(),
        Some("synthetic clone fixture")
    );
}
