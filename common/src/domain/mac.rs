use itertools::{EitherOrBoth, Itertools};
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};
#[cfg(feature = "sqlx")]
use sqlx::{Database, Decode};
use std::{
    convert::TryInto,
    fmt::{self, Display},
    str::FromStr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MacAddr {
    V6(MacAddr6),
    V8(MacAddr8),
}

impl MacAddr {
    fn bytes(&self) -> &[u8] {
        match self {
            MacAddr::V6(b) => &b.0,
            MacAddr::V8(b) => &b.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MacAddr6([u8; 6]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MacAddr8([u8; 8]);

impl Serialize for MacAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            serializer.collect_str(&format_args!("{}", self))
            // let mut last_ok = None;
            // for s in Itertools::intersperse(self.bytes().iter().map(|b| byte_to_str(*b)), [b':', 0])
            // {
            //     let len = 1 + (s[1] != 0) as usize;
            //     let string = unsafe { from_utf8_unchecked(&s[..len]) };
            //     last_ok = Some(string.serialize(&serializer)?);
            // }
            // Ok(last_ok.unwrap())
        } else {
            serializer.serialize_bytes(self.bytes())
        }
    }
}

// fn byte_to_str(b: u8) -> [u8; 2] {
//     fn nible_to_ascii(n: u8) -> u8 {
//         debug_assert!(n < 0x10);
//         if n < 0xa {
//             n + b'0'
//         } else {
//             n + b'a' - 0xa
//         }
//     }
//     [nible_to_ascii(b >> 4), nible_to_ascii(b & 0xf)]
// }

struct MacVisitor;

impl<'de> Visitor<'de> for MacVisitor {
    type Value = MacAddr;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a mac address with 6 or 8 bytes")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match v.len() {
            6 => Ok(MacAddr::V6(MacAddr6(v.try_into().unwrap()))),
            8 => Ok(MacAddr::V8(MacAddr8(v.try_into().unwrap()))),
            l => Err(E::invalid_length(l, &"between 6 and 8 bytes of input")),
        }
    }

    fn visit_borrowed_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let mut buf = [0; 8];
        for (i, element) in v.split(':').zip_longest(&mut buf).enumerate() {
            match element {
                EitherOrBoth::Both(s, b) => {
                    *b = match u8::from_str_radix(s, 16) {
                        Ok(b) => b,
                        Err(_) => {
                            return Err(E::invalid_type(de::Unexpected::Other(s), &"byte (0-255)"))
                        }
                    };
                }
                // I still have string values but no more slots in buffer.
                EitherOrBoth::Left(_) => {
                    return Err(E::invalid_length(i, &"between 6 and 8 bytes"));
                }
                // string is done.
                EitherOrBoth::Right(&mut 0) if i == 6 => {
                    let [v6 @ .., _, _] = buf;
                    return Ok(MacAddr::V6(MacAddr6(v6)));
                }
                EitherOrBoth::Right(_) => {
                    return Err(E::invalid_length(i, &"between 6 and 8 bytes"));
                }
            }
        }
        Ok(MacAddr::V8(MacAddr8(buf)))
    }
}

impl<'de> Deserialize<'de> for MacAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(MacVisitor)
        } else {
            deserializer.deserialize_bytes(MacVisitor)
        }
    }
}

impl Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            &self
                .bytes()
                .iter()
                .format_with(":", |e, f| f(&format_args!("{:02x}", e)))
        )
    }
}

//TODO: delete?
#[cfg(feature = "sqlx")]
impl<'r, DB: Database> Decode<'r, DB> for MacAddr
where
    &'r str: Decode<'r, DB>,
{
    fn decode(
        value: <DB as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let v = <&str as Decode<DB>>::decode(value)?;
        Ok(serde_json::from_str(v)?)
    }
}

impl FromStr for MacAddr {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MacVisitor.visit_borrowed_str(s)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // #[test]
    // fn byte_to_str_works() {
    //     for i in 0..=u8::MAX {
    //         let expect = format!("{:02x}", i);
    //         let buf = byte_to_str(i);
    //         let is = unsafe { from_utf8_unchecked(&buf) };
    //         assert_eq!(expect, is);
    //     }
    // }

    #[test]
    fn json_parse_mac_v8() {
        let mac = MacAddr::V8(MacAddr8([0xdd, 0xee, 0xaa, 0xdd, 0xbb, 0xee, 0xee, 0xff]));
        let s = serde_json::to_string(&mac).unwrap();
        assert_eq!(s, "\"dd:ee:aa:dd:bb:ee:ee:ff\"");
        assert_eq!(mac, serde_json::from_str(&s).unwrap());
    }

    #[test]
    fn json_parse_mac_v6() {
        let mac = MacAddr::V6(MacAddr6([0xff, 0xaa, 0xcc, 0xaa, 0xdd, 0xaa]));
        let s = serde_json::to_string(&mac).unwrap();
        assert_eq!(s, "\"ff:aa:cc:aa:dd:aa\"");
        assert_eq!(mac, serde_json::from_str(&s).unwrap());
    }
}
