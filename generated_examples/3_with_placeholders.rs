#[test]
fn generated_api_sequence() {
    let __PLACEHOLDER_0__ = base64::Engine::decode_vec(__PARAM_0_0__, __PARAM_0_1__);
    let __PLACEHOLDER_1__ = base64::GeneralPurposeConfig::with_decode_allow_trailing_bits(__PARAM_1_0__);
    let __PLACEHOLDER_2__ = base64::Engine::encode_slice(__PARAM_2_0__, __PARAM_2_1__);
    let __PLACEHOLDER_3__ = base64::Engine::encode_string(__PARAM_3_0__, __PARAM_3_1__);
    let __PLACEHOLDER_4__ = base64::EncoderStringWriter::new(__PARAM_4_0__);
    let __PLACEHOLDER_5__ = base64::EncoderWriter::new(__PARAM_5_0__, __PARAM_5_1__);
    let __PLACEHOLDER_6__ = base64::EncoderStringWriter::into_inner();
    let __PLACEHOLDER_7__ = base64::GeneralPurposeConfig::with_encode_padding(__PARAM_7_0__);
    let __PLACEHOLDER_8__ = base64::Engine::decode(__PARAM_8_0__);
    let __PLACEHOLDER_9__ = base64::DecoderReader::new(__PARAM_9_0__, __PARAM_9_1__);
}
