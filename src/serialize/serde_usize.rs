use std::marker::PhantomData;

use serde::{de::Visitor, Deserializer, Serializer};

pub trait AsUsize: Copy {
    fn as_usize(self) -> usize;
}

pub trait FromUsize: Copy {
    fn from_usize(value: usize) -> Self;
}

impl<T> AsUsize for *const T {
    fn as_usize(self) -> usize {
        self as usize
    }
}

impl<T> FromUsize for *const T {
    fn from_usize(value: usize) -> Self {
        value as Self
    }
}

pub fn serialize<S, T>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Copy + AsUsize,
{
    let value = (*value).as_usize();
    serializer.serialize_u64(value as u64)
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromUsize,
{
    struct V<Ptr>(PhantomData<Ptr>);

    impl<'de, Ptr> Visitor<'de> for V<Ptr>
    where
        Ptr: FromUsize,
    {
        type Value = Ptr;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a size type")
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Self::Value::from_usize(v as usize))
        }
    }

    deserializer.deserialize_u64(V::<T>(PhantomData))
}
