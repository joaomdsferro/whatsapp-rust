use std::mem::{align_of, size_of};
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
