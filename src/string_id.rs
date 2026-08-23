/// Declare a bidirectional enum/string boundary from one correlation table.
///
/// The enum remains the in-process representation. The generated string
/// functions exist only for parsing input and presenting output, and cannot
/// drift because both directions are expanded from the same entries.
#[macro_export]
macro_rules! string_id_map {
    (
        $type:ty,
        $table:ident,
        $to_string:ident,
        $from_string:ident {
            $(
                $variant:path => $wire:literal $(| $alias:literal)*
            ),+ $(,)?
        }
    ) => {
        impl $type {
            pub const $table: &'static [(Self, &'static str)] = &[
                $(($variant, $wire)),+
            ];

            pub const fn $to_string(self) -> &'static str {
                match self {
                    $($variant => $wire),+
                }
            }

            pub fn $from_string(wire: &str) -> Option<Self> {
                match wire {
                    $($wire $(| $alias)* => Some($variant)),+,
                    _ => None,
                }
            }
        }
    };
}

/// Attach serde to a string-mapped enum without introducing another mapping.
#[macro_export]
macro_rules! string_id_serde {
    ($type:ty, $to_string:ident, $from_string:ident) => {
        impl serde::Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(self.$to_string())
            }
        }

        impl<'de> serde::Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let wire = <String as serde::Deserialize>::deserialize(deserializer)?;
                Self::$from_string(&wire).ok_or_else(|| {
                    serde::de::Error::custom(format_args!(
                        "unknown {} value {wire:?}",
                        stringify!($type)
                    ))
                })
            }
        }
    };
}
