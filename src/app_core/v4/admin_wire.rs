//! One strict decoder pattern for validated schema 3 administrative records.
macro_rules! validated_admin {
    ($ty:ident { $($field:ident: $type:ty),* $(,)? }) => {
        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D:serde::Deserializer<'de>>(d:D)->Result<Self,D::Error> {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Wire { $($field:$type),* }
                let w=Wire::deserialize(d).map_err(|_|serde::de::Error::custom("invalid v4 contract"))?;
                let value=Self { $($field:w.$field),* };
                value.validate().map_err(serde::de::Error::custom)?;
                Ok(value)
            }
        }
        impl std::fmt::Debug for $ty {
            fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { f.write_str(concat!(stringify!($ty),"([redacted])")) }
        }
    }
}
