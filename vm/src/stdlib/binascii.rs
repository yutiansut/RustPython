use crate::function::OptionalArg;
use crate::obj::objbytearray::{PyByteArray, PyByteArrayRef};
use crate::obj::objbyteinner::PyBytesLike;
use crate::obj::objbytes::{PyBytes, PyBytesRef};
use crate::obj::objstr::{PyString, PyStringRef};
use crate::pyobject::{PyObjectRef, PyResult, TryFromObject, TypeProtocol};
use crate::vm::VirtualMachine;

use crc::{crc32, Hasher32};
use itertools::Itertools;

enum SerializedData {
    Bytes(PyBytesRef),
    Buffer(PyByteArrayRef),
    Ascii(PyStringRef),
}

impl TryFromObject for SerializedData {
    fn try_from_object(vm: &VirtualMachine, obj: PyObjectRef) -> PyResult<Self> {
        match_class!(match obj {
            b @ PyBytes => Ok(SerializedData::Bytes(b)),
            b @ PyByteArray => Ok(SerializedData::Buffer(b)),
            a @ PyString => {
                if a.as_str().is_ascii() {
                    Ok(SerializedData::Ascii(a))
                } else {
                    Err(vm.new_value_error(
                        "string argument should contain only ASCII characters".to_string(),
                    ))
                }
            }
            obj => Err(vm.new_type_error(format!(
                "argument should be bytes, buffer or ASCII string, not '{}'",
                obj.class().name,
            ))),
        })
    }
}

impl SerializedData {
    #[inline]
    pub fn with_ref<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        match self {
            SerializedData::Bytes(b) => f(b.get_value()),
            SerializedData::Buffer(b) => f(&b.inner.borrow().elements),
            SerializedData::Ascii(a) => f(a.as_str().as_bytes()),
        }
    }
}

fn hex_nibble(n: u8) -> u8 {
    match n {
        0..=9 => b'0' + n,
        10..=15 => b'a' + n,
        _ => unreachable!(),
    }
}

fn binascii_hexlify(data: PyBytesLike, _vm: &VirtualMachine) -> Vec<u8> {
    data.with_ref(|bytes| {
        let mut hex = Vec::<u8>::with_capacity(bytes.len() * 2);
        for b in bytes.iter() {
            hex.push(hex_nibble(b >> 4));
            hex.push(hex_nibble(b & 0xf));
        }
        hex
    })
}

fn unhex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn binascii_unhexlify(data: SerializedData, vm: &VirtualMachine) -> PyResult<Vec<u8>> {
    data.with_ref(|hex_bytes| {
        if hex_bytes.len() % 2 != 0 {
            return Err(vm.new_value_error("Odd-length string".to_string()));
        }

        let mut unhex = Vec::<u8>::with_capacity(hex_bytes.len() / 2);
        for (n1, n2) in hex_bytes.iter().tuples() {
            if let (Some(n1), Some(n2)) = (unhex_nibble(*n1), unhex_nibble(*n2)) {
                unhex.push(n1 << 4 | n2);
            } else {
                return Err(vm.new_value_error("Non-hexadecimal digit found".to_string()));
            }
        }

        Ok(unhex)
    })
}

fn binascii_crc32(data: SerializedData, value: OptionalArg<u32>, vm: &VirtualMachine) -> PyResult {
    let crc = value.unwrap_or(0);

    let mut digest = crc32::Digest::new_with_initial(crc32::IEEE, crc);
    data.with_ref(|bytes| digest.write(&bytes));

    Ok(vm.ctx.new_int(digest.sum32()))
}

fn binascii_a2b_base64(s: SerializedData, vm: &VirtualMachine) -> PyResult<Vec<u8>> {
    s.with_ref(|bytes| {
        base64::decode(bytes)
            .map_err(|err| vm.new_value_error(format!("error decoding base64: {}", err)))
    })
}

fn binascii_b2a_base64(data: PyBytesLike, _vm: &VirtualMachine) -> Vec<u8> {
    data.with_ref(|b| base64::encode(b).into_bytes())
}

pub fn make_module(vm: &VirtualMachine) -> PyObjectRef {
    let ctx = &vm.ctx;

    py_module!(vm, "binascii", {
        "hexlify" => ctx.new_rustfunc(binascii_hexlify),
        "b2a_hex" => ctx.new_rustfunc(binascii_hexlify),
        "unhexlify" => ctx.new_rustfunc(binascii_unhexlify),
        "a2b_hex" => ctx.new_rustfunc(binascii_unhexlify),
        "crc32" => ctx.new_rustfunc(binascii_crc32),
        "a2b_base64" => ctx.new_rustfunc(binascii_a2b_base64),
        "b2a_base64" => ctx.new_rustfunc(binascii_b2a_base64),
    })
}
