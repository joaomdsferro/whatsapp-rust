/// Internal owner that allocates its collection header only for future fields.
#[repr(transparent)]
#[derive(Default)]
pub struct Storage(Option<Box<::buffa::UnknownFields>>);

static EMPTY: ::buffa::UnknownFields = ::buffa::UnknownFields::new();

impl PartialEq for Storage {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl ::core::hash::Hash for Storage {
    fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {
        ::core::hash::Hash::hash(&**self, state);
    }
}

impl ::core::fmt::Debug for Storage {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        ::core::fmt::Debug::fmt(&**self, f)
    }
}
impl Clone for Storage {
    #[inline]
    fn clone(&self) -> Self {
        if self.is_empty() {
            Self::default()
        } else {
            Self::from(clone_nonempty(self))
        }
    }
}
#[cold]
#[inline(never)]
fn clone_nonempty(value: &::buffa::UnknownFields) -> ::buffa::UnknownFields {
    value.clone()
}

impl Drop for Storage {
    #[inline]
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            drop_storage(value);
        }
    }
}
#[cold]
#[inline(never)]
fn drop_storage(value: Box<::buffa::UnknownFields>) {
    drop(value);
}
impl ::core::ops::Deref for Storage {
    type Target = ::buffa::UnknownFields;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0.as_deref().unwrap_or(&EMPTY)
    }
}
impl ::core::ops::DerefMut for Storage {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.get_or_insert_with(Box::default)
    }
}
impl From<::buffa::UnknownFields> for Storage {
    #[inline]
    fn from(value: ::buffa::UnknownFields) -> Self {
        Self((!value.is_empty()).then(|| Box::new(value)))
    }
}
impl From<Storage> for ::buffa::UnknownFields {
    #[inline]
    fn from(value: Storage) -> Self {
        let mut value = value;
        value.0.take().map(|fields| *fields).unwrap_or_default()
    }
}
impl PartialEq<::buffa::UnknownFields> for Storage {
    fn eq(&self, other: &::buffa::UnknownFields) -> bool {
        &**self == other
    }
}
impl PartialEq<Storage> for ::buffa::UnknownFields {
    fn eq(&self, other: &Storage) -> bool {
        self == &**other
    }
}
impl<'a> IntoIterator for &'a Storage {
    type Item = &'a ::buffa::UnknownField;
    type IntoIter = ::core::slice::Iter<'a, ::buffa::UnknownField>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl IntoIterator for Storage {
    type Item = ::buffa::UnknownField;
    type IntoIter = <::buffa::UnknownFields as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        ::buffa::UnknownFields::from(self).into_iter()
    }
}
impl Storage {
    /// Collection header owned outside the message's inline pointer slot.
    pub fn heap_header_bytes(&self) -> usize {
        if self.0.is_some() {
            ::core::mem::size_of::<::buffa::UnknownFields>()
        } else {
            0
        }
    }

    #[cold]
    #[inline(never)]
    pub(super) fn merge_unknown(
        &mut self,
        tag: ::buffa::encoding::Tag,
        buf: &mut impl ::buffa::bytes::Buf,
        ctx: ::buffa::DecodeContext<'_>,
    ) -> Result<(), ::buffa::DecodeError> {
        self.push(::buffa::encoding::decode_unknown_field(tag, buf, ctx)?);
        Ok(())
    }
    #[inline]
    pub fn encoded_len(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            unknown_len(self)
        }
    }
    #[inline]
    pub fn write_to(&self, buf: &mut impl ::buffa::EncodeSink) {
        if !self.is_empty() {
            write_unknown(self, buf);
        }
    }
    #[inline]
    pub fn clear(&mut self) {
        if !self.is_empty() {
            clear_unknown(self);
        }
    }
    #[cold]
    #[inline(never)]
    pub fn push(&mut self, field: ::buffa::UnknownField) {
        self.0.get_or_insert_with(Box::default).push(field);
    }
}
#[cold]
#[inline(never)]
fn unknown_len(value: &::buffa::UnknownFields) -> usize {
    value.encoded_len()
}
#[cold]
#[inline(never)]
fn write_unknown(value: &::buffa::UnknownFields, buf: &mut impl ::buffa::EncodeSink) {
    value.write_to(buf);
}
#[cold]
#[inline(never)]
fn clear_unknown(value: &mut ::buffa::UnknownFields) {
    value.clear();
}
