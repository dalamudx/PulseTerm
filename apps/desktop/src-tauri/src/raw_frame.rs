//! Typed binary Channel payload; never serialized through Serde.
use specta::{Type, Types, datatype::DataType};
use tauri::ipc::{InvokeResponseBody, IpcResponse};

pub(crate) struct RawFrame(pub(crate) Vec<u8>);

impl IpcResponse for RawFrame {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Raw(self.0))
    }
}

impl Type for RawFrame {
    fn definition(_: &mut Types) -> DataType {
        // This opaque type deliberately has no JSON representation or JS transform.
        DataType::Reference(specta_typescript::define("ArrayBuffer"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn payload_is_raw_not_a_json_array() {
        let bytes = vec![0, 255, 27, 128];
        match RawFrame(bytes.clone()).body().unwrap() {
            InvokeResponseBody::Raw(actual) => assert_eq!(actual, bytes),
            InvokeResponseBody::Json(_) => panic!("binary payload was serialized"),
        }
    }
}
