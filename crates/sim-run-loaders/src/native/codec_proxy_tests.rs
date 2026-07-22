use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, ReadPolicy, Symbol, WriteCx};

use super::{NativeAbiCodecDecoder, NativeAbiCodecEncoder, NativeGuest};

// conformance: native loader codec proxy preserves manifest and export payloads.

// A stand-in guest decodes the marshaled frame and returns a canned response,
// so the proxy's marshal/unmarshal bridge is exercised without a real dylib.
struct MockGuest;

impl NativeGuest for MockGuest {
    fn invoke(&self, op: &str, args: &[u8]) -> sim_kernel::Result<Vec<u8>> {
        let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), args)?;
        if op.ends_with("/decode") {
            assert!(
                matches!(expr, Expr::String(_)),
                "decode op should receive the input text as Expr::String"
            );
            Ok(sim_codec_binary::encode_frame(&Expr::Symbol(Symbol::new("parsed")))?.0)
        } else {
            Ok(sim_codec_binary::encode_frame(&Expr::String("rendered".to_owned()))?.0)
        }
    }
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

#[test]
fn proxy_decoder_marshals_text_to_guest_and_returns_expr() {
    let decoder = NativeAbiCodecDecoder {
        guest: Arc::new(MockGuest),
        op: "codec/mock/decode".to_owned(),
    };
    let mut cx = test_cx();
    let mut rcx = sim_codec::ReadCx {
        cx: &mut cx,
        codec: sim_kernel::CodecId(0),
        read_policy: ReadPolicy::default(),
        limits: sim_codec::DecodeLimits::default(),
    };
    let expr = sim_codec::Decoder::decode(
        &decoder,
        &mut rcx,
        sim_codec::Input::Text("(hello)".to_owned()),
    )
    .unwrap();
    assert_eq!(expr, Expr::Symbol(Symbol::new("parsed")));
}

#[test]
fn proxy_encoder_marshals_expr_to_guest_and_returns_text() {
    let encoder = NativeAbiCodecEncoder {
        guest: Arc::new(MockGuest),
        op: "codec/mock/encode".to_owned(),
    };
    let mut cx = test_cx();
    let mut wcx = WriteCx {
        cx: &mut cx,
        codec: sim_kernel::CodecId(0),
        options: sim_kernel::EncodeOptions::default(),
    };
    let out =
        sim_codec::Encoder::encode(&encoder, &mut wcx, &Expr::Symbol(Symbol::new("x"))).unwrap();
    assert_eq!(out, sim_codec::Output::Text("rendered".to_owned()));
}
